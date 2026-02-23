# Summit

Peer-to-peer encrypted communication for Linux. Direct device-to-device computing, messaging, and file transfer over IPv6 link-local -- no routers, no DNS, no infrastructure.

## How It Works

Devices discover each other via IPv6 multicast, establish encrypted sessions using [Noise_XX](http://www.noiseprotocol.org/), and exchange typed, content-addressed chunks. Trust is cryptographic: capability hashes identify services, content hashes verify data, and public keys verify peers.

## Components

| Crate | Description |
|-------|-------------|
| `summitd` | Daemon -- capability discovery, session management, chunk cache, network I/O |
| `summit-ctl` | CLI tool -- inspect status, manage trust, send files |
| `summit-core` | Wire format, cryptography (Noise_XX, BLAKE3), message types |
| `summit-services` | Service layer -- cache, trust registry, QoS, file transfer |
| `summit-api` | HTTP/REST API (Axum) for daemon control |
| `zenith` | Desktop UI (Electron) |

## Installation

### From Release (Recommended)

Download the latest release from [Releases](../../releases):

```bash
# x86_64
tar xzf summit-x86_64-unknown-linux-gnu.tar.gz
sudo mv summitd summit-ctl /usr/local/bin/

# ARM64
tar xzf summit-aarch64-unknown-linux-gnu.tar.gz
sudo mv summitd summit-ctl /usr/local/bin/
```

### From Source

```bash
cargo build --release -p summitd -p summit-ctl
sudo cp target/release/{summitd,summit-ctl} /usr/local/bin/
```

### Docker

```bash
docker pull ghcr.io/ivy/summit:latest
docker run -it --rm --privileged --network host ghcr.io/ivy/summit:latest
```

## Usage

Start the daemon on a network interface:

```bash
sudo summitd eth0        # wired
sudo summitd wlp5s0      # wireless
```

Control with `summit-ctl`:

```bash
summit-ctl status                     # daemon status, sessions, cache
summit-ctl peers                      # discovered peers
summit-ctl trust add <pubkey>         # trust a peer
summit-ctl trust block <pubkey>       # block a peer
summit-ctl trust pending              # peers awaiting trust
summit-ctl send file.pdf              # broadcast to all trusted peers
summit-ctl send file.pdf --peer <key> # send to specific peer
summit-ctl files                      # list received files
summit-ctl cache                      # cache stats
summit-ctl sessions inspect <id>      # session details
summit-ctl shutdown                   # stop daemon
summit-ctl messages send <pubkey> 'hello world'
summit-ctl compute submit <pubkey> '{"cmd":"echo","args":["hi"]}'
summit-ctl compute tasks <pubkey>
```
  # ── Machine A (submitter) ──────────────────────────────

  # 1. Verify both daemons see each other
  summit-ctl peers

  # 2. Confirm compute is enabled
  summit-ctl services

  # 3. Copy Machine B's public key from peers output, submit a task
  summit-ctl compute submit <B_pubkey> '{"cmd":"echo","args":["hello"]}'

  # 4. Check task status — should now show "Queued" (ack received from B)
  summit-ctl compute tasks


  # ── Machine B (receiver) ───────────────────────────────

  # 1. List ALL incoming compute tasks (no pubkey needed)
  summit-ctl compute tasks

  # 2. Or filter by sender
  summit-ctl compute tasks <A_pubkey>

## Design

- **Capabilities, not addresses** -- services identified by cryptographic hash, not IP/port
- **Typed chunks** -- self-describing transport with schema IDs and content hashing
- **Symmetric sessions** -- no client/server distinction, both peers contribute equally
- **Content-addressed caching** -- deduplication and zero-copy reads via mmap
- **QoS contracts** -- Realtime (never buffered), Bulk (high throughput), Background (low priority)
- **Three-tier trust** -- Trusted (full access), Untrusted (chunks buffered), Blocked (rejected)

## License

MIT
