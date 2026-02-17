# Summit

**Encrypted peer-to-peer file transfer over local networks**

Summit is a high-performance P2P protocol built for reliable file transfer across local WiFi and Ethernet networks. It provides encrypted communication, automatic peer discovery, content-addressed caching, and multipath redundancy.

---

## Features

- **Zero-configuration peer discovery** via IPv6 link-local multicast
- **End-to-end encryption** using Noise_XX with ephemeral key exchange
- **Content-addressed chunk storage** with BLAKE3 hashing
- **Multipath redundancy** — same chunk sent via multiple routes simultaneously
- **QoS token bucket rate limiting** per session contract (Realtime/Bulk/Background)
- **Automatic file reassembly** from chunked transfers
- **Schema validation** with pluggable validators
- **HTTP API** for programmatic control
- **CLI tool** (`summit-ctl`) for user interaction

---

## Quick Start

### Prerequisites

- Linux (tested on Ubuntu 24)
- Rust 1.83+ with 2024 edition support
- Root access for network namespace tests

### Build

```bash
cargo build --release
```

### Basic Usage

**Terminal 1 — Start daemon:**
```bash
sudo ./target/release/summitd eth0
```

**Terminal 2 — Send a file:**
```bash
./target/release/summit-ctl send myfile.pdf
```

**Terminal 3 — Check status:**
```bash
./target/release/summit-ctl status
./target/release/summit-ctl files
```

Files are automatically received and reassembled in `/tmp/summit-received/`.

---

## Architecture

### Protocol Stack

```
┌─────────────────────────────────────────┐
│  Application Layer                      │
│  - File chunking/reassembly             │
│  - Schema validation                    │
└─────────────────────────────────────────┘
            ↓
┌─────────────────────────────────────────┐
│  Session Layer (Noise_XX)               │
│  - ChaCha20-Poly1305 encryption         │
│  - Forward secrecy                      │
│  - Mutual authentication                │
└─────────────────────────────────────────┘
            ↓
┌─────────────────────────────────────────┐
│  Transport Layer (UDP)                  │
│  - Ephemeral ports per session          │
│  - Separate session/chunk channels      │
└─────────────────────────────────────────┘
            ↓
┌─────────────────────────────────────────┐
│  Discovery Layer (Multicast)            │
│  - ff02::1 capability announcements     │
│  - Link-local scope                     │
└─────────────────────────────────────────┘
```

### Core Components

#### 1. Discovery (`capability/`)

Peers announce themselves via IPv6 multicast to `ff02::1:9000`:

```rust
pub struct CapabilityAnnouncement {
    pub capability_hash: [u8; 32],    // What you support
    pub public_key:      [u8; 32],    // Your identity
    pub session_port:    u16,         // Where to handshake
    pub chunk_port:      u16,         // Announced during handshake
    pub version:         u32,
    pub contract:        u8,          // Realtime/Bulk/Background
}
```

- **Broadcast every 2 seconds**
- **60-second TTL** for discovered peers
- **Registry keyed by public key** to prevent self-discovery

#### 2. Session Establishment (`session/`)

Uses **Noise_XX** for authenticated key exchange:

**Handshake Flow:**
```
Initiator                     Responder
   |                             |
   |  HandshakeInit (msg1)       |
   |  [nonce, ephemeral_pub]     |
   |---------------------------->|
   |                             |
   |  HandshakeResponse (msg2)   |
   |  [nonce, ephemeral+static]  |
   |<----------------------------|
   |                             |
   |  HandshakeComplete (msg3)   |
   |  [static pub, proof]        |
   |---------------------------->|
   |                             |
   |  Encrypted chunk_port       |
   |<--------------------------->|
   |                             |
  [Session established - ChaCha20-Poly1305 ready]
```

**Key features:**
- **Deterministic initiator selection**: Lower public key initiates
- **Single session listener** with HandshakeTracker state machine
- **Ephemeral ports** prevent conflicts
- **Separate sockets** for session handshake vs. chunk I/O

#### 3. File Transfer (`transfer.rs`)

Files are chunked, sent, and reassembled automatically:

**Sending:**
```rust
File (any size)
  ↓
Split into 32KB chunks
  ↓
Generate metadata chunk:
  - filename
  - total_bytes
  - chunk_hashes[]
  ↓
Queue metadata + data chunks
  ↓
Send worker broadcasts to all sessions
```

**Receiving:**
```rust
Receive metadata chunk (type_tag=3)
  ↓
Track in FileReassembler
  ↓
Receive data chunks (type_tag=2)
  ↓
Match by content hash
  ↓
When all chunks received → reassemble
  ↓
Write to /tmp/summit-received/
```

#### 4. Content-Addressed Cache (`cache.rs`)

Git-style storage with BLAKE3 hashing:

```
/tmp/summit-cache-<PID>/
├── c4/
│   └── c43e92ba...cdbc92  (chunk file)
├── e0/
│   └── e079e5f0...9cab8a
└── ...
```

- **Automatic deduplication** — same content stored once
- **Cache-on-send** — chunks cached before transmission
- **Cache-on-receive** — received chunks cached immediately
- **Multipath-safe** — duplicate deliveries detected by hash

#### 5. QoS Rate Limiting (`qos.rs`)

Token bucket per session enforces contract limits:

| Contract   | Refill Rate | Burst Size | Use Case                |
|------------|-------------|------------|-------------------------|
| Realtime   | Unlimited   | Unlimited  | Audio, telemetry        |
| Bulk       | 64/sec      | 32         | File transfer (default) |
| Background | 8/sec       | 4          | Replication, indexing   |

**Priority rules:**
- Background suppressed when Realtime sessions active
- Tokens refill based on elapsed time
- Empty bucket = drop packet

#### 6. Schema Validation (`schema.rs`)

Pluggable validators ensure payload integrity:

```rust
pub enum KnownSchema {
    TestPing,      // UTF-8 "ping #N"
    TextMessage,   // UTF-8 text
    FileChunk,     // Raw bytes
    FileData,      // Raw bytes (32KB chunks)
    FileMetadata,  // JSON with filename, hashes
}
```

Chunks rejected if validation fails.

---

## Wire Format

All on-wire structs use `zerocopy` for safe, zero-copy parsing:

### CapabilityAnnouncement (80 bytes)
```
 0                   1                   2                   3
 0 1 2 3 4 5 6 7 8 9 0 1 2 3 4 5 6 7 8 9 0 1 2 3 4 5 6 7 8 9 0 1
+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
|                     capability_hash (32 bytes)                 |
+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
|                       public_key (32 bytes)                    |
+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
|          session_port         |          chunk_port           |
+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
|                           version                             |
+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
|    contract   |
+-+-+-+-+-+-+-+-+
```

### ChunkHeader (72 bytes + payload)
```
 0                   1                   2                   3
 0 1 2 3 4 5 6 7 8 9 0 1 2 3 4 5 6 7 8 9 0 1 2 3 4 5 6 7 8 9 0 1
+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
|                     content_hash (32 bytes)                    |
+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
|                      schema_id (32 bytes)                      |
+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
|    type_tag   |    flags      |           version             |
+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
|                           length                              |
+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
|                         payload (variable)                     |
+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
```

Entire header + payload encrypted with ChaCha20-Poly1305.

---

## API Reference

### HTTP Endpoints

The daemon exposes a REST API on `127.0.0.1:9001`:

#### `GET /status`
Returns daemon status, active sessions, cache stats.

**Response:**
```json
{
  "sessions": [
    {
      "session_id": "da7c9d1d...",
      "peer": "[fe80::44c0...]:50215",
      "contract": "Bulk",
      "chunk_port": 47564,
      "established_secs": 42
    }
  ],
  "cache": {
    "chunks": 12,
    "bytes": 387200
  },
  "peers_discovered": 3
}
```

#### `GET /peers`
Lists discovered peers from multicast announcements.

**Response:**
```json
{
  "peers": [
    {
      "public_key": "045686d1...",
      "addr": "fe80::78cf:5bff:fe03:af6",
      "session_port": 57487,
      "chunk_port": 56286,
      "contract": 2,
      "version": 1,
      "last_seen_secs": 5
    }
  ]
}
```

#### `GET /cache`
Cache statistics.

**Response:**
```json
{
  "chunks": 12,
  "bytes": 387200
}
```

#### `POST /cache/clear`
Clears all cached chunks.

**Response:**
```json
{
  "cleared": 12
}
```

#### `POST /send`
Upload file (multipart/form-data), chunk it, queue for sending.

**Response:**
```json
{
  "filename": "document.pdf",
  "bytes": 524288,
  "chunks_sent": 17
}
```

#### `GET /files`
Lists received files.

**Response:**
```json
{
  "received": ["document.pdf", "image.png"],
  "in_progress": ["large_file.zip"]
}
```

### CLI Commands

#### `summit-ctl status`
Show daemon status, sessions, cache.

#### `summit-ctl peers`
List discovered peers with last-seen times.

#### `summit-ctl cache`
Display cache statistics (chunks, bytes).

#### `summit-ctl cache clear`
Clear all cached chunks.

#### `summit-ctl send <file>`
Upload and broadcast file to all connected peers.

#### `summit-ctl files`
List received files and in-progress transfers.

---

## Security Model

### Cryptographic Primitives

- **BLAKE3**: Content hashing and schema IDs
- **Noise_XX**: Session key exchange
  - X25519 for Diffie-Hellman
  - ChaCha20-Poly1305 for AEAD
- **Ed25519**: Static identity keys (via `snow`)

### Threat Model

**Protects against:**
- ✅ Eavesdropping (all chunks encrypted)
- ✅ Tampering (AEAD authentication tags)
- ✅ Replay attacks (nonces, ephemeral keys)
- ✅ Man-in-the-middle (mutual authentication)
- ✅ Content corruption (BLAKE3 verification)

**Does NOT protect against:**
- ❌ Traffic analysis (peer discovery is plaintext multicast)
- ❌ Denial of service (UDP, no rate limiting on handshake)
- ❌ Trust-on-first-use attacks (no PKI or key pinning)

### Privacy Considerations

- **Link-local only**: Traffic never leaves local network segment
- **Ephemeral ports**: Reduces fingerprinting
- **Content-addressed**: No filename metadata in chunks (except in FileMetadata type)
- **No persistent logs**: Daemon uses structured logging to stdout

---

## Performance Characteristics

### Throughput

- **Single session**: Limited by UDP + encryption overhead (~500 Mbps on gigabit)
- **Multipath**: Linear scaling with session count (2 sessions ≈ 1 Gbps)
- **Chunk size**: 32 KB (tuned for UDP MTU and cache efficiency)

### Latency

- **Session establishment**: ~5ms (3-way Noise handshake)
- **File transfer start**: ~10ms (metadata chunk + first data chunk)
- **Cache lookup**: <1ms (filesystem-backed, no network)

### Resource Usage

- **Memory**: ~5 MB base + 100 KB per session
- **Disk**: Git-style cache grows unbounded (clear with `summit-ctl cache clear`)
- **Network**: 2-second multicast announcements + actual data transfer

---

## Development

### Project Structure

```
summit/
├── crates/
│   ├── summit-core/          # Wire types, crypto, shared code
│   │   ├── src/
│   │   │   ├── crypto.rs     # Noise_XX, BLAKE3, keypair
│   │   │   └── wire.rs       # Zerocopy structs
│   │   └── Cargo.toml
│   ├── summitd/              # Main daemon
│   │   ├── src/
│   │   │   ├── main.rs       # Task orchestration
│   │   │   ├── cache.rs      # Content-addressed storage
│   │   │   ├── capability/   # Discovery, broadcast, listener
│   │   │   ├── chunk/        # Send, receive loops
│   │   │   ├── delivery.rs   # Multipath tracking
│   │   │   ├── qos.rs        # Token bucket rate limiting
│   │   │   ├── schema.rs     # Validation
│   │   │   ├── session/      # Handshake, state machine
│   │   │   ├── status.rs     # HTTP API
│   │   │   └── transfer.rs   # File chunking/reassembly
│   │   └── Cargo.toml
│   ├── summit-ctl/           # CLI tool
│   │   ├── src/main.rs
│   │   └── Cargo.toml
│   └── libsummit/            # (Reserved for future C FFI)
├── tests/integration/        # Network namespace tests
├── scripts/
│   ├── netns-up.sh           # Create test namespaces
│   └── netns-down.sh         # Cleanup
└── Cargo.toml                # Workspace
```

### Running Tests

**Unit tests:**
```bash
cargo test --lib
```

**Integration tests (requires root):**
```bash
sudo ./scripts/netns-up.sh
sudo cargo test --test integration
```

Network namespace tests verify:
- Peer discovery across namespaces
- Session establishment
- File transfer end-to-end
- Cache operations

### Adding a New Schema

1. Add variant to `schema::KnownSchema`
2. Implement `id()` to return `hash(b"your.schema.name")`
3. Implement `name()` to return string
4. Add validator function if needed
5. Update `validator()` match

Example:
```rust
pub enum KnownSchema {
    // existing...
    JsonMetadata,  // NEW
}

impl KnownSchema {
    pub fn id(&self) -> [u8; 32] {
        match self {
            // existing...
            Self::JsonMetadata => hash(b"summit.json.metadata"),
        }
    }
    
    pub fn validator(&self) -> Option<Box<dyn Fn(&[u8]) -> bool + Send + Sync>> {
        match self {
            // existing...
            Self::JsonMetadata => Some(Box::new(validate_json_metadata)),
        }
    }
}

fn validate_json_metadata(payload: &[u8]) -> bool {
    serde_json::from_slice::<YourType>(payload).is_ok()
}
```

---

## Roadmap

### Completed (v0.1)
- [x] Noise_XX encrypted sessions
- [x] Multicast peer discovery
- [x] Content-addressed caching
- [x] Schema validation
- [x] Multipath delivery tracking
- [x] QoS rate limiting
- [x] File transfer with automatic reassembly
- [x] HTTP API + CLI tool
- [x] Integration tests

### Planned
- [ ] **WiFi Direct support** (GOAL-06)
- [ ] Prometheus metrics export
- [ ] Persistent peer identity (key pinning)
- [ ] NAT traversal (STUN/TURN)
- [ ] Streaming audio/video contracts
- [ ] Mobile SDKs (iOS/Android via libsummit)
- [ ] Web UI dashboard

---

## Troubleshooting

### Sessions not establishing

**Check peer discovery:**
```bash
summit-ctl peers
```

If no peers listed:
- Verify multicast is enabled on interface
- Check firewall rules for UDP port 9000
- Ensure both nodes on same link-local segment

**Check logs:**
```bash
RUST_LOG=debug summitd eth0
```

Look for "session established" messages.

### File transfer fails

**Verify session exists:**
```bash
summit-ctl status
```

**Check cache:**
```bash
summit-ctl cache
```

**Review daemon logs** for "file transfer started" and "file completed" messages.

### High peer count but no sessions

This indicates a **session race condition**. Both nodes discovered each other but handshake failed due to:
- Simultaneous initiation (should be prevented by key comparison)
- Network packet loss during 3-way handshake

**Workaround:** Restart daemon to trigger new handshake attempt.

---

## License

MIT License - see LICENSE file for details.

---

## Contributing

We welcome contributions! Please:

1. Open an issue for discussion before major changes
2. Follow existing code style (rustfmt)
3. Add tests for new features
4. Update documentation

---

## Acknowledgments

Built with:
- [snow](https://github.com/mcginty/snow) - Noise protocol implementation
- [blake3](https://github.com/BLAKE3-team/BLAKE3) - Cryptographic hashing
- [zerocopy](https://github.com/google/zerocopy) - Safe zero-copy parsing
- [tokio](https://github.com/tokio-rs/tokio) - Async runtime
- [axum](https://github.com/tokio-rs/axum) - HTTP framework

Inspired by:
- IPFS (content addressing)
- Noise Protocol Framework (authenticated encryption)
- BitTorrent (multipath redundancy)
