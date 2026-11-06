# Summit

Devices that can physically reach each other talk directly: messages, files
and compute, end-to-end encrypted, with no servers, no accounts, no internet
and no configuration.

## What "direct" means

Summit runs on a single network **link**: whatever carries frames between two
machines. It speaks IPv6 link-local only, so its traffic is never routed:
nothing above that link is involved, and nothing beyond it can reach in.

The link can be:

- **an Ethernet cable** between two machines: nothing in between at all
- **Wi-Fi Direct / ad-hoc Wi-Fi**: two radios, no access point
- **USB tethering or a phone hotspot**: the phone is the link
- **a switch or Wi-Fi access point**: they forward frames between the machines
  but take no part in Summit (a router never sees the traffic, and there is
  no internet hop). Networks with "client isolation" (guest/public Wi-Fi)
  refuse to forward between clients, and Summit can't work there.

If two devices share a link, they find each other automatically. If they
don't, Summit doesn't try to bridge the gap.

## What Summit is not

Summit stays small on purpose. It is **not**:

- a way to reach devices across the internet (no relays, no NAT traversal)
- an account system or identity provider: a device *is* its key pair
- a social network, app store or plugin platform
- a general-purpose overlay network

New features have to fit "devices on the same link, talking directly".

## How It Works

Devices announce themselves with IPv6 link-local multicast, establish
encrypted sessions using [Noise_XX](http://www.noiseprotocol.org/), and
exchange typed, content-addressed chunks. Trust is cryptographic: a peer's
identity is the static key it proves during the handshake, content hashes
verify data, and capability hashes identify services. If one side can't
receive multicast (common on phones), the session still forms: whoever can
hear the other initiates.

## Components

| Crate | Description |
|-------|-------------|
| `libsummit` | Embeddable node -- discovery, sessions, chunk transport, local API (what `summitd` and future mobile/desktop hosts run) |
| `summitd` | Daemon -- thin CLI wrapper around `libsummit` |
| `summit-ctl` | CLI tool -- inspect status, manage trust, send files |
| `summit-core` | Wire format, cryptography (Noise_XX, BLAKE3), message types |
| `summit-services` | Service layer -- cache, trust registry, QoS, file transfer |
| `summit-api` | HTTP/REST API (Axum) for daemon control |
| `zenith` | Desktop UI (Electron) |

## Installation

### From Release (Recommended)

Download the latest release from [Releases](https://github.com/4-R-C-4-N-4/summit/releases):

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

Start the daemon on the interface for the link you want to use (no root
needed):

```bash
summitd eth0             # cable / switch
summitd wlp5s0           # Wi-Fi
summitd p2p-wlp5s0-0     # Wi-Fi Direct group interface
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
summit-ctl compute submit <pubkey> -- "hostnamectl"
# Responses with stdout provided as text response
summit-ctl compute submit <pubkey> -- "hostnamectl > info.txt"
# files generated are automatically broadcast back to sender
summit-ctl compute tasks             # List all distributed tasks
summit-ctl compute tasks <pubkey>    # List distributed tasks per peer
```
## Example: distributed computing
```
  # ── Machine A (submitter) ──────────────────────────────

  # 1. Verify both daemons see each other
  summit-ctl peers

  # 2. Confirm compute is enabled
  summit-ctl services

  # 3. Copy Machine B's public key from peers output, submit a task
  summit-ctl compute submit <B_pubkey> "hostnamectl > info.txt"

  # 4. Check task status — should now show "Queued" (ack received from B)
  summit-ctl compute tasks


  # ── Machine B (receiver) ───────────────────────────────

  # 1. List ALL incoming compute tasks (no pubkey needed)
  summit-ctl compute tasks

  # 2. Or filter by sender
  summit-ctl compute tasks <A_pubkey>
```
## Zenith UI

<img width="2664" height="1918" alt="image" src="https://github.com/user-attachments/assets/242eff40-f32f-4df2-99d8-1c2d696058c5" />
<img width="2664" height="1918" alt="image" src="https://github.com/user-attachments/assets/2386ea9c-db5a-4937-88d4-d6e645fdf595" />


## Design

- **Capabilities, not addresses** -- services identified by cryptographic hash, not IP/port
- **Typed chunks** -- self-describing transport with schema IDs and content hashing
- **Symmetric sessions** -- no client/server distinction, both peers contribute equally
- **Content-addressed caching** -- deduplication and zero-copy reads via mmap
- **QoS contracts** -- Realtime (never buffered), Bulk (high throughput), Background (low priority)
- **Three-tier trust** -- Trusted (full access), Untrusted (chunks buffered), Blocked (rejected)

## Consider helping me support this project forever
[Kofi time?](https://ko-fi.com/4rc4n4)
BTC: 34qdTbWkTDDs1LhdseuC3JrknaydqFryvg

## License

MIT
