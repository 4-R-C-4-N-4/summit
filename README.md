# Summit

**Direct. Verified. Infrastructureless.**

Summit is a peer-to-peer communication protocol and daemon for Linux that enables direct, encrypted, capability-addressed communication between proximal devices — no router, no DNS, no certificate authority, no infrastructure of any kind required.

Two devices. One cable, or one radio link. That's enough.

---

## The Problem With How Devices Talk Today

Every mainstream networking protocol was designed around a central assumption: communication happens between identified endpoints, mediated by infrastructure. You find a device by its address. You trust its identity because a certificate authority vouches for it. You route traffic through access points, routers, and DNS resolvers that you don't control and can't fully trust.

This model made sense when networks were sparse and hardware was slow. It no longer reflects reality. Two laptops sitting on the same desk are fully capable of talking directly to each other at hundreds of megabits per second, with cryptographic verification that no third party can match. But the software stack forces them to communicate as if they were on opposite sides of the planet, routing packets through infrastructure that adds latency, fragility, and attack surface.

Summit is built on a different assumption: **proximity is a first-class property, and trust should be mathematical, not institutional.**

---

## Core Design Principles

### 1. Capabilities, Not Addresses

In Summit, you do not connect to an address. You announce what you need, and devices that can provide it respond.

Every service is identified by a **capability hash** — a cryptographic hash of a capability descriptor that specifies what the service does, what schema it speaks, and what version it is. A device broadcasting a capability is simultaneously identifying the service, versioning it, and providing the verification key. Connecting to the wrong device is cryptographically impossible — if the hash doesn't match, the session doesn't open.

This eliminates service discovery as a separate problem, eliminates a whole class of man-in-the-middle attacks, and makes versioning structural rather than a convention.

### 2. Typed, Self-Describing Chunks — Not Byte Streams

TCP gives you a raw byte stream. Every protocol ever built on top of TCP then reinvents framing — headers, length prefixes, delimiters — because the transport has no concept of message boundaries or data types.

Summit's transport primitive is the **chunk**: a small, fixed header containing a schema ID, content hash, type tag, and length, followed by a payload. The receiver always knows what it's getting, how long it is, and how to verify it before reading the payload. Partial chunks are safe to discard. Chunks of different types can be multiplexed on the same session without head-of-line blocking.

The schema ID is itself a content hash of a schema definition, negotiated at session establishment. Both sides speak the same typed language from the first exchange.

### 3. Symmetric Sessions — No Client, No Server

TCP has an initiator and a responder. This asymmetry is load-bearing in the current internet stack — firewalls assume it, NAT assumes it, APIs assume it. It also means that peer-to-peer communication is an exception you have to engineer around rather than the natural default.

In Summit, sessions are established by both parties contributing a nonce and a public key, producing a shared session ID that neither party controls unilaterally. Either side can send at any time. Either side can propose schema upgrades. The concepts of "client" and "server" are application-level concerns — the transport layer has no opinion about them.

### 4. Content-Addressed Chunks With Opportunistic Caching

Because every chunk carries a hash of its own content, the transport layer can deduplicate and cache chunks transparently, without any application-level coordination.

If a device requests a chunk that is already present in the local cache — because it was received in a previous session, or from a different peer — it is served from cache without a network round trip. If two peers are exchanging a large payload and one already has part of it, the chunk hashes reveal this automatically. No application-level diff or sync protocol required.

The chunk cache is a memory-mapped region shared between the daemon and applications. Cache hits require zero syscalls and zero data copies — the kernel's virtual memory system does all the work.

### 5. Latency Contracts, Not Best-Effort

Rather than treating all traffic identically and leaving congestion control entirely to the application, Summit sessions declare a **communication contract** at establishment time:

- **Realtime** — latency-sensitive, loss-tolerant (audio, input events, telemetry)
- **Bulk** — throughput-optimized, loss-intolerant (file transfer, sync)
- **Background** — low-priority, interruptible (replication, indexing)

The daemon makes scheduling and buffering decisions based on these contracts. Realtime chunks are never queued behind a bulk transfer. This replaces the current situation where every application layer reinvents congestion control and buffering from scratch.

### 6. Schema-Driven Wire Encoding

If both sides have negotiated a shared schema, the wire encoding can be radically more efficient than general-purpose serialization formats. Fixed-width fields are encoded at fixed width. Repeated structures are delta-encoded. Strings defined in the schema are interned. The result is Protocol Buffers-level wire efficiency with no code generation step and no out-of-band schema distribution — the schema hash in every chunk header *is* the schema reference.

---

## Trust Without Infrastructure

The deepest design goal of Summit is this: **verification should be structural, not infrastructural.**

In the current stack, trust is institutional. You trust a server because a certificate authority signed its certificate. You trust a DNS response because your ISP's resolver says so. You trust the content of a packet because the router it came through is probably not malicious.

In Summit, trust is mathematical. The capability hash tells you exactly what service you're connecting to. The Noise protocol session tells you the specific device you're talking to, and that nobody in the middle can read or modify the exchange. The chunk content hash tells you the data is exactly what was sent. None of this requires any third party. None of this requires any infrastructure. Two devices with a physical link between them have everything they need.

This is not a marginal improvement in security. It is a qualitatively different security property — one that falls out naturally from the design rather than requiring bolted-on certificates, signed packages, or trusted registries.

---

## Physical Layer

Summit runs over any link that provides an IP interface. No special hardware is required.

**WiFi Direct (IEEE 802.11 P2P)** is the primary wireless transport. It is supported by essentially every WiFi chipset manufactured in the last decade, requires no access point or router, and provides the full bandwidth of the underlying WiFi standard. Summit manages WiFi Direct link establishment through `wpa_supplicant`'s P2P interface, presenting a ready network interface to the protocol stack above.

**Direct Ethernet** (crossover or switch-free) is the primary wired transport and the recommended development environment. Modern NICs handle crossover automatically. Combined with IPv6 link-local addressing, a single cable between two machines is sufficient to run a full Summit session with no configuration.

**Ad-hoc WiFi (IBSS)** is supported as a fallback for hardware that does not support WiFi Direct.

IPv6 link-local addresses (`fe80::/10`) are used for all direct communication. These are assigned automatically by the kernel to every network interface the moment a link comes up — no DHCP, no configuration, no infrastructure.

---

## Architecture

```
┌─────────────────────────────────────┐
│     Applications (Rust or C)        │
├─────────────────────────────────────┤
│         libsummit                   │  Rust crate + C FFI via cbindgen
│                                     │  capability lookup, chunk send/recv,
│                                     │  zero-copy cache access
├─────────────────────────────────────┤
│         summitd                     │  Rust daemon — capability registry,
│                                     │  session management, chunk cache,
│                                     │  Noise_XX via snow, schema negotiation
├──────────────┬──────────────────────┤
│  tokio-uring │   UDP multicast      │  io_uring async I/O + IPv6 link-local
│  (io_uring)  │   ff02::1            │  multicast for discovery broadcast
├──────────────┴──────────────────────┤
│           Linux Kernel              │
└─────────────────────────────────────┘
```

**`summitd`** is the core daemon, written in Rust. It owns the capability registry, manages Noise protocol session establishment, maintains the content-addressed chunk cache as a shared memory-mapped region, and handles all network I/O through `io_uring` via `tokio-uring` for maximum throughput with minimal syscall overhead.

**`libsummit`** is the Rust library that applications link against, with a C FFI layer generated automatically by `cbindgen` for non-Rust consumers. It provides a clean API expressed entirely in terms of capabilities and typed chunks. Applications never interact with sockets, addresses, or session keys directly.

**`summit-ctl`** is the command-line control and inspection tool, written in Rust via `clap`. It communicates with `summitd` over a Unix domain socket and exposes the capability registry, active sessions, cache statistics, and link status in human-readable or JSON form.

---

## Zenith — v0.1 Milestone

The first release milestone is **Zenith**. Zenith is intentionally scoped: a working proof of the core design over a direct ethernet link between two machines running Arch Linux.

Zenith delivers:

- Capability announcement and discovery over IPv6 link-local multicast
- Noise\_XX session establishment between two peers
- Chunk framing with schema negotiation and content hash verification
- Basic content-addressed chunk cache with mmap'd application access
- WiFi Direct link establishment via `wpa_supplicant` P2P
- `summit-ctl` for session and cache inspection

Zenith does not deliver multi-hop routing, mesh networking, or non-Linux platform support. Those come later. Zenith proves the foundation.

---

## What Summit Is Not

Summit is not a replacement for the internet. It does not route across the public internet, does not replace TCP/IP for general use, and does not provide anonymity.

Summit is a communication substrate for devices that are physically near each other and want to talk directly, with strong cryptographic guarantees, no dependency on shared infrastructure, and a cleaner design than the 40-year-old abstractions currently doing this job.

---

## Status

**Pre-release. Active development toward Zenith (v0.1).**

Summit is designed for Linux. Development is conducted on Arch Linux against the current upstream kernel. Contributions, design discussions, and protocol feedback are welcome.

---

## License

TBD — likely MIT or Apache 2.0. Chosen before Zenith release.

---

*Summit. Because the best path between two points is a straight line.*
