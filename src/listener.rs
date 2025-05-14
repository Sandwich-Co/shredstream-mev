use log::{error, info};
use std::io::Write;
use std::sync::Arc;
use tokio::net::UdpSocket;
use tokio::signal;
use tokio::sync::{Mutex, RwLock};
use tokio::time::{sleep, Duration};

use crate::arb::PoolsState;
use crate::benchmark::Sigs;
use crate::entry_processor::{ArbEntryProcessor, PumpEntryProcessor};
use crate::graduates_processor::GraduatesProcessor;
use crate::service::Mode;
use crate::shred_processor::ShredProcessor;

pub const PACKET_SIZE: usize = 1280 - 40 - 8;

pub async fn listen(
    socket: Arc<UdpSocket>,
    received_packets: Arc<Mutex<Vec<Vec<u8>>>>,
) {
    let mut buf = [0u8; PACKET_SIZE]; // max shred size
    loop {
        match socket.recv_from(&mut buf).await {
            Ok((received, _)) => {
                let packet = Vec::from(&buf[..received]);
                received_packets.lock().await.push(packet);
            }
            Err(e) => {
                error!("Error receiving packet: {:?}", e);
            }
        }
    }
}

pub async fn dump_to_file(received_packets: Arc<Mutex<Vec<Vec<u8>>>>) {
    let packets = received_packets.lock().await;
    let mut file =
        std::fs::File::create("packets.json").expect("Couldn't create file");
    let as_json = serde_json::to_string(&packets.clone())
        .expect("Couldn't serialize to json");
    file.write_all(as_json.as_bytes())
        .expect("Couldn't write to file");
    info!("Packets dumped to packets.json");
}

pub async fn run_listener_with_algo(
    bind_addr: &str,
    shreds_sigs: Option<Sigs>,
    mode: Mode,
    post_url: String,
    benchmark: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    let socket = Arc::new(
        UdpSocket::bind(bind_addr)
            .await
            .expect("Couldn't bind to address"),
    );
    let (entry_tx, entry_rx) = tokio::sync::mpsc::channel(2000);
    let (error_tx, error_rx) = tokio::sync::mpsc::channel(2000);
    let (sig_tx, mut sig_rx) = tokio::sync::mpsc::channel(2000);
    let shred_processor =
        Arc::new(RwLock::new(ShredProcessor::new(entry_tx, error_tx)));

    info!("Listening on {}", bind_addr);

    // metrics loop
    info!("Starting metrics loop");
    let shred_processor_clone = shred_processor.clone();
    tokio::spawn(async move {
        loop {
            sleep(Duration::from_secs(6)).await;
            {
                let metrics = shred_processor_clone.read().await.metrics();
                info!("metrics: {}", metrics);
                drop(metrics);
            }
        }
    });

    info!("Starting shred processor");
    let mut buf = [0u8; PACKET_SIZE]; // max shred size
    let shred_processor = shred_processor.clone();
    tokio::spawn(async move {
        loop {
            match socket.recv_from(&mut buf).await {
                Ok((received, _)) => {
                    let packet = Vec::from(&buf[..received]);
                    shred_processor
                        .write()
                        .await
                        .collect(Arc::new(packet))
                        .await;
                }
                Err(e) => {
                    error!("Error receiving packet: {:?}", e);
                }
            }
        }
    });

    info!("Starting entry processor");
    match mode {
        Mode::Arb => tokio::spawn(async move {
            info!("Starting entries rx (<=> webhook tx) arb mode");
            let pools_state = Arc::new(RwLock::new(PoolsState::default()));
            pools_state.write().await.initialize().await;
            let mut entry_processor = ArbEntryProcessor::new(
                entry_rx,
                error_rx,
                pools_state.clone(),
                sig_tx,
            );
            entry_processor.receive_entries().await;
        }),
        Mode::Pump => {
            info!("Starting entries rx (<=> webhook tx) pump mode");
            tokio::spawn(async move {
                let mut entry_processor = PumpEntryProcessor::new(
                    entry_rx, error_rx, sig_tx, post_url,
                );
                entry_processor.receive_entries().await;
            })
        }
        Mode::Graduates => {
            info!("Starting entries rx (<=> webhook tx) graduates mode");
            tokio::spawn(async move {
                let mut entry_processor =
                    GraduatesProcessor::new(entry_rx, error_rx, sig_tx);
                entry_processor.receive_entries().await;
            })
        }
        Mode::All => tokio::spawn(async move {
            let (arb_entry_tx, arb_entry_rx) =
                tokio::sync::mpsc::channel(2000);
            let (pump_entry_tx, pump_entry_rx) =
                tokio::sync::mpsc::channel(2000);
            let (grad_entry_tx, grad_entry_rx) =
                tokio::sync::mpsc::channel(2000);

            let (arb_error_tx, arb_error_rx) =
                tokio::sync::mpsc::channel(2000);
            let (pump_error_tx, pump_error_rx) =
                tokio::sync::mpsc::channel(2000);
            let (grad_error_tx, grad_error_rx) =
                tokio::sync::mpsc::channel(2000);

            tokio::spawn(async move {
                let mut rx: tokio::sync::mpsc::Receiver<
                    crate::entry_processor::EntriesWithMeta,
                > = entry_rx;
                while let Some(msg) = rx.recv().await {
                    let _ = arb_entry_tx.send(msg.clone()).await;
                    let _ = pump_entry_tx.send(msg.clone()).await;
                    let _ = grad_entry_tx.send(msg).await;
                }
            });

            tokio::spawn(async move {
                let mut rx: tokio::sync::mpsc::Receiver<String> = error_rx;
                while let Some(err) = rx.recv().await {
                    let _ = arb_error_tx.send(err.clone()).await;
                    let _ = pump_error_tx.send(err.clone()).await;
                    let _ = grad_error_tx.send(err).await;
                }
            });

            let sig_tx_arb = sig_tx.clone();
            let sig_tx_pump = sig_tx.clone();
            let sig_tx_grad = sig_tx;

            let arb_handle = tokio::spawn(async move {
                info!("Starting entries rx (<=> webhook tx) arb mode");
                let pools_state =
                    Arc::new(RwLock::new(PoolsState::default()));
                pools_state.write().await.initialize().await;
                let mut entry_processor = ArbEntryProcessor::new(
                    arb_entry_rx,
                    arb_error_rx,
                    pools_state.clone(),
                    sig_tx_arb,
                );
                entry_processor.receive_entries().await;
            });

            let pump_handle = tokio::spawn(async move {
                info!("Starting entries rx (<=> webhook tx) pump mode");
                let mut entry_processor = PumpEntryProcessor::new(
                    pump_entry_rx,
                    pump_error_rx,
                    sig_tx_pump,
                    post_url,
                );
                entry_processor.receive_entries().await;
            });

            let grads_handle = tokio::spawn(async move {
                info!("Starting entries rx (<=> webhook tx) graduates mode");
                let mut entry_processor = GraduatesProcessor::new(
                    grad_entry_rx,
                    grad_error_rx,
                    sig_tx_grad,
                );
                entry_processor.receive_entries().await;
            });

            let _ = tokio::join!(arb_handle, pump_handle, grads_handle);
        }),
    };

    info!("Starting sigs loop");
    tokio::spawn({
        let shreds_sigs = shreds_sigs.clone();
        async move {
            while let Some(sig) = sig_rx.recv().await {
                if benchmark {
                    if let Some(shreds_sigs) = &shreds_sigs {
                        let timestamp = chrono::Utc::now().timestamp_millis();
                        info!("algo: {} {}", timestamp, sig);
                        shreds_sigs
                            .write()
                            .await
                            .push((timestamp as u64, sig.clone()));
                    }
                }
            }
        }
    });

    signal::ctrl_c().await.expect("Failed to listen for Ctrl+C");

    Ok(())
}

pub async fn run_listener_with_save(
    bind_addr: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    let socket = Arc::new(
        UdpSocket::bind(bind_addr)
            .await
            .expect("Couldn't bind to address"),
    );
    let received_packets = Arc::new(Mutex::new(Vec::new()));

    info!("Listening on {}", bind_addr);
    let rx = received_packets.clone();
    let socket_clone = socket.clone();

    tokio::spawn(async move {
        listen(socket_clone, rx).await;
    });

    loop {
        let packets = received_packets.lock().await;
        info!("Total packets received: {}", packets.len());
        if packets.len() > 100_000 {
            info!("Dumping packets to file");
            break;
        }
        drop(packets);
        sleep(Duration::from_secs(1)).await;
    }
    dump_to_file(received_packets).await;
    Ok(())
}
