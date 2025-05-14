# ShredStream Traction Decoder

A high-performance listener and processor for Solana gossip-protocol shreds, plus integration with Raydium and custom “pump” and “graduates” modes. Includes a “proxy” for jito-shredstream, a main `shreds` service, and a simple CLI.

## Table of Contents

1. [Prerequisites](#prerequisites)  
2. [Directory Layout](#directory-layout)  
3. [Configuration](#configuration)  
4. [Running with Docker Compose](#running-with-docker-compose)  
   - [Proxy Service](#proxy-service)  
   - [Shreds Service](#shreds-service)  
   - [CLI Service](#cli-service)  
5. [Common Troubleshooting](#common-troubleshooting)  
6. [Development & Testing](#development--testing)  
7. [License](#license)

---

## Prerequisites

- Docker & Docker Compose  
- `rust` (for local development)  
- A Solana keypair JSON (for the proxy service) at `./auth.json`  
- `raydium.json` file in project root (downloaded via CLI or manually)  

---

## Directory Layout

```

.
├── Cargo.toml
├── Dockerfile
├── docker-compose.yaml
├── src/               # Rust source
│   ├── app.rs
│   ├── listener.rs
│   ├── shred\_processor.rs
│   ├── service.rs
│   └── …
├── auth.json          # Jito shredstream auth keypair
├── raydium.json       # Raydium pool data
├── packets.json       # (auto-generated test data)
└── FAST.json          # (keypair for Raydium SDK)

````

---

## Configuration

Populate the following files in the project root:

- **`auth.json`**: Your Solana keypair for the jito-shredstream proxy.  
- **`raydium.json`**: Download via:
  ```bash
  cargo run --release --bin shreds download
````

* **`.env`** (optional):

  ```
  RPC_URL=https://api.mainnet-beta.solana.com
  WS_URL=wss://api.mainnet-beta.solana.com
  FUND_KEYPAIR_PATH=/path/to/FAST.json
  ```

---

## Running with Docker Compose

All three services (proxy, shreds, cli) can be managed together.

```bash
docker-compose build
docker-compose up proxy shreds cli
```

### Proxy Service

Streams Solana shreds via Jito:

```bash
docker-compose up proxy
```

* **Env vars** (in `docker-compose.yaml`):

  * `BLOCK_ENGINE_URL`
  * `AUTH_KEYPAIR` (mounted from `./auth.json`)
  * `DESIRED_REGIONS`
  * `DEST_IP_PORTS`

### Shreds Service

Main listener + processor in “graduates” mode by default:

```bash
docker-compose up shreds
```

* Binds UDP `0.0.0.0:8001` (host network)
* Processes incoming shreds into FEC sets, recovers entries, and sends to webhook at default `http://0.0.0.0:6969`

Override mode via subcommand:

```bash
docker-compose run --rm shreds ./shreds arb-mode --bind 0.0.0.0:8001 --post-url http://example.com
```

### CLI Service

Helper / debugging CLI:

```bash
docker-compose up cli
```

Shows `--help` and allows you to invoke any subcommand manually:

```bash
docker-compose run --rm cli ./shreds save --bind 0.0.0.0:8001
```

---

## Common Troubleshooting

* **“Error getting shred id”**
  Occurs if incoming UDP packets aren’t valid Solana shreds. Verify your source or log raw packet bytes.
* **Health check `/healthz` refused**
  Make sure your webhook endpoint (default `http://0.0.0.0:6969/healthz`) is running before starting `shreds`.
* **Port conflicts**
  If `0.0.0.0:8001` is in use, change the bind port via `--bind` or in `docker-compose.yaml`.
* **File mount errors**
  Ensure `auth.json`, `FAST.json`, and `raydium.json` exist and are files (not directories), owned by your user.

---

## Development & Testing

* **Local build & run (no Docker)**

  ```bash
  cargo build --release
  ./target/release/shreds save --bind 0.0.0.0:8001
  ```

* **Run tests**

  ```bash
  cargo test
  ```

* **Generate test packets**

  The `Save` mode will write `packets.json` once >100,000 packets are collected:

  ```bash
  ./target/release/shreds save --bind 0.0.0.0:8001
  # wait until dump
  ```

  Then tests in `shred_processor.rs` can consume it.

---

## License

MIT