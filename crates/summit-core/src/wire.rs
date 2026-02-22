//! Summit wire format — on-wire types for all Summit communication.
//!
//! These types ARE the protocol. Every field, every size, every reserved byte
//! is part of the wire format. Changing anything here after Zenith is a
//! breaking change. Read docs/wire-format.md before modifying.
//!
//! All types are #[repr(C, packed)] for deterministic layout and use
//! zerocopy derives for safe, allocation-free serialization. There is no
//! unsafe code in this module.

use static_assertions::assert_eq_size;
use zerocopy::{AsBytes, FromBytes, FromZeroes};

// ── Chunk Header ─────────────────────────────────────────────────────────────

/// The atomic unit of Summit communication.
///
/// Every payload transmitted in Summit is preceded by this header.
/// The receiver can fully describe, verify, and route a chunk before
/// reading a single byte of payload.
///
/// Wire size: 72 bytes.
#[derive(Debug, Clone, AsBytes, FromBytes, FromZeroes)]
#[repr(C, packed)]
pub struct ChunkHeader {
    /// BLAKE3 hash of the payload bytes.
    /// Verified by the receiver before the chunk is accepted or cached.
    /// A mismatch silently discards the chunk — no error is sent.
    pub content_hash: [u8; 32],

    /// BLAKE3 hash of the schema definition that describes this chunk's payload.
    /// Must be present in the negotiated schema set for this session.
    /// Use SCHEMA_ID_RAW ([0u8; 32]) for untyped raw chunks.
    pub schema_id: [u8; 32],

    /// Application-defined chunk type within the schema.
    /// Interpretation is schema-specific. The transport does not inspect this.
    pub type_tag: u16,

    /// Length of the payload in bytes, not including this header.
    /// Maximum payload: 65535 bytes. Larger data must be split by the sender.
    pub length: u32,

    /// Bit flags:
    ///   bits 0-1: priority class (mirrors Contract — 0x01 realtime, 0x02 bulk, 0x03 background)
    ///   bit    2: payload is zstd-compressed (reserved, not implemented in Zenith)
    ///   bits 3-7: reserved, must be zero
    pub flags: u8,

    /// Wire format version. Currently 0x01.
    /// A receiver seeing an unknown version silently drops the chunk.
    pub version: u8,
}

// Compile-time size guard. If this fails, the wire format has silently changed.
assert_eq_size!(ChunkHeader, [u8; 72]);

// ── Service Hashes ────────────────────────────────────────────────────────────

/// Service identifier — BLAKE3 hash of a canonical service name.
/// Used in CapabilityAnnouncement.service_hash and for routing.
pub type ServiceHash = [u8; 32];

/// Compute a ServiceHash from a canonical name.
/// The input byte string is the protocol-level name and must never change
/// for a given service after Zenith.
pub fn service_hash(name: &[u8]) -> ServiceHash {
    *blake3::hash(name).as_bytes()
}

/// Pre-computed service hash functions. Recomputed on each call (no allocation).
pub fn file_transfer_hash() -> ServiceHash {
    service_hash(b"summit.file_transfer")
}

pub fn messaging_hash() -> ServiceHash {
    service_hash(b"summit.messaging")
}

pub fn stream_udp_hash() -> ServiceHash {
    service_hash(b"summit.stream_udp")
}

pub fn compute_hash() -> ServiceHash {
    service_hash(b"summit.compute")
}

// ── Capability Announcement ───────────────────────────────────────────────────

/// Broadcast via ff02::1 multicast to announce ONE service.
///
/// Peers send N datagrams per broadcast interval, one per enabled service.
/// Receivers collect datagrams by `public_key` and build the peer's full
/// service set when `service_index` values 0..service_count-1 are all present.
///
/// Wire size: 76 bytes (was 74 — two new fields: service_count, service_index).
#[derive(Debug, Clone, AsBytes, FromBytes, FromZeroes)]
#[repr(C, packed)]
pub struct CapabilityAnnouncement {
    /// BLAKE3 hash identifying which service this datagram announces.
    /// Was `capability_hash` — renamed for clarity.
    pub service_hash: [u8; 32],

    /// Ed25519 public key of the announcing peer.
    pub public_key: [u8; 32],

    /// Protocol version.
    pub version: u32,

    /// Session handshake port (TCP).
    pub session_port: u16,

    /// Chunk data port. 0 = use session_port (typical for Bulk services).
    /// For Realtime services (future), a dedicated UDP port.
    pub chunk_port: u16,

    /// Contract for THIS service.
    /// 0x01 = Realtime, 0x02 = Bulk, 0x03 = Background.
    pub contract: u8,

    /// Bit flags. Reserved, must be zero.
    pub flags: u8,

    /// Total number of services this peer offers.
    pub service_count: u8,

    /// Zero-indexed position of this service in the broadcast set.
    pub service_index: u8,
}

assert_eq_size!(CapabilityAnnouncement, [u8; 76]);

// ── Handshake ─────────────────────────────────────────────────────────────────

/// Noise_XX handshake message 1 — sent by the session initiator.
///
/// The initiator sends this to the responder's session_port after discovering
/// the capability in the peer registry. It carries the initiator's ephemeral
/// key (Noise message 1), a copy of the announcement being responded to so
/// the responder can verify which capability is being requested, and a nonce
/// that contributes to the derived session ID.
///
/// Wire size: 120 bytes.
/// Noise_XX handshake message 1 — sent by the session initiator.
/// Wire size: 80 bytes
#[derive(Debug, Clone, AsBytes, FromBytes, FromZeroes)]
#[repr(C, packed)]
pub struct HandshakeInit {
    /// Initiator nonce — contributes to session ID derivation.
    pub nonce: [u8; 16],
    /// The service hash being requested.
    pub service_hash: [u8; 32],
    /// Raw Noise_XX message 1 bytes — passed directly to snow.
    pub noise_msg: [u8; 32],
}

assert_eq_size!(HandshakeInit, [u8; 80]);

/// Noise_XX handshake message 2 — sent by the responder.
/// Wire size: 112 bytes (16 nonce + 96 noise message)
#[derive(Debug, Clone, AsBytes, FromBytes, FromZeroes)]
#[repr(C, packed)]
pub struct HandshakeResponse {
    /// Responder nonce — contributes to session ID derivation.
    pub nonce: [u8; 16],
    /// Raw Noise_XX message 2 bytes — exactly 96 bytes for Noise_XX.
    pub noise_msg: [u8; 96],
}

assert_eq_size!(HandshakeResponse, [u8; 112]);

/// Noise_XX handshake message 3 — sent by the initiator.
/// Wire size: 64 bytes (Noise_XX msg3 exact size for 25519_ChaChaPoly_BLAKE2s)
#[derive(Debug, Clone, AsBytes, FromBytes, FromZeroes)]
#[repr(C, packed)]
pub struct HandshakeComplete {
    /// Raw Noise_XX message 3 bytes — passed directly to snow.
    pub noise_msg: [u8; 64],
}

assert_eq_size!(HandshakeComplete, [u8; 64]);

// ── Contract ──────────────────────────────────────────────────────────────────

/// Latency contract — declared per session, governs scheduling.
///
/// The daemon uses the contract to make buffering and scheduling decisions.
/// A Realtime chunk is never queued behind a Bulk transfer.
/// Applications choose the contract that matches their data's latency tolerance.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum Contract {
    /// Low latency, loss-tolerant. Audio, input events, telemetry.
    /// Chunks are never buffered. Prefer freshness over completeness.
    Realtime = 0x01,

    /// High throughput, loss-intolerant. File transfer, sync.
    /// Chunks are buffered and retransmitted until acknowledged.
    Bulk = 0x02,

    /// Low priority, interruptible. Replication, background indexing.
    /// Transmitted only when no Realtime or Bulk traffic is active.
    Background = 0x03,
}

impl TryFrom<u8> for Contract {
    type Error = WireError;

    fn try_from(value: u8) -> Result<Self, Self::Error> {
        match value {
            0x01 => Ok(Contract::Realtime),
            0x02 => Ok(Contract::Bulk),
            0x03 => Ok(Contract::Background),
            other => Err(WireError::UnknownContract(other)),
        }
    }
}

impl From<Contract> for u8 {
    fn from(c: Contract) -> u8 {
        c as u8
    }
}

// ── Constants ─────────────────────────────────────────────────────────────────

/// Schema ID used for raw, untyped chunks.
/// Chunks with this schema ID bypass schema validation entirely.
pub const SCHEMA_ID_RAW: [u8; 32] = [0u8; 32];

/// Current chunk format version.
pub const CHUNK_VERSION: u8 = 0x01;

/// Maximum payload size in bytes.
/// Larger data must be split by the sender into multiple chunks.
pub const MAX_PAYLOAD: usize = 65535;

/// IPv6 link-local multicast address for capability announcements.
pub const MULTICAST_ADDR: &str = "ff02::1";

/// Default capability announcement interval in seconds.
pub const ANNOUNCE_INTERVAL_SECS: u64 = 2;

/// Default peer registry TTL in seconds.
/// Peers not seen within this window are removed from the registry.
pub const PEER_TTL_SECS: u64 = 10;

/// Default handshake timeout in seconds.
/// Incomplete handshakes are cleaned up after this interval.
pub const HANDSHAKE_TIMEOUT_SECS: u64 = 5;

// ── Errors ────────────────────────────────────────────────────────────────────

/// Errors that can arise when interpreting wire-format data.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum WireError {
    #[error("unknown contract byte: 0x{0:02x}")]
    UnknownContract(u8),

    #[error("unknown chunk version: 0x{0:02x}")]
    UnknownVersion(u8),

    #[error("payload length {0} exceeds maximum {}", MAX_PAYLOAD)]
    PayloadTooLarge(usize),

    #[error("reserved flags are non-zero: 0x{0:02x}")]
    ReservedFlagsSet(u8),
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use zerocopy::AsBytes;

    #[allow(dead_code)]
    fn zeroed_chunk_header() -> ChunkHeader {
        ChunkHeader {
            content_hash: [0u8; 32],
            schema_id: [0u8; 32],
            type_tag: 0,
            length: 0,
            flags: 0,
            version: CHUNK_VERSION,
        }
    }

    #[test]
    fn chunk_header_round_trip() {
        let original = ChunkHeader {
            content_hash: [0xab; 32],
            schema_id: [0xcd; 32],
            type_tag: 0x0102,
            length: 1024,
            flags: 0x01,
            version: CHUNK_VERSION,
        };

        let bytes = original.as_bytes();
        assert_eq!(bytes.len(), 72);

        let recovered = ChunkHeader::read_from(bytes).unwrap();
        assert_eq!(recovered.content_hash, original.content_hash);
        assert_eq!(recovered.schema_id, original.schema_id);
        // type_tag and length are packed — read via copy to avoid unaligned access
        let type_tag: u16 = u16::from_ne_bytes(bytes[64..66].try_into().unwrap());
        let length: u32 = u32::from_ne_bytes(bytes[66..70].try_into().unwrap());
        assert_eq!(type_tag, 0x0102);
        assert_eq!(length, 1024);
        assert_eq!(recovered.flags, original.flags);
        assert_eq!(recovered.version, original.version);
    }

    #[test]
    fn announcement_round_trip() {
        let original = CapabilityAnnouncement {
            service_hash: [0x11; 32],
            public_key: [0x22; 32],
            version: 7,
            session_port: 9000,
            chunk_port: 9001,
            contract: Contract::Bulk as u8,
            flags: 0,
            service_count: 3,
            service_index: 1,
        };

        let bytes = original.as_bytes();
        assert_eq!(bytes.len(), 76);

        let recovered = CapabilityAnnouncement::read_from(bytes).unwrap();

        // Copy packed fields to locals to avoid unaligned reference UB
        let recovered_service_hash = recovered.service_hash;
        let recovered_public_key = recovered.public_key;
        let recovered_session_port = recovered.session_port;
        let recovered_chunk_port = recovered.chunk_port;
        let recovered_contract = recovered.contract;
        let recovered_version = recovered.version;
        let recovered_service_count = recovered.service_count;
        let recovered_service_index = recovered.service_index;

        assert_eq!(recovered_service_hash, original.service_hash);
        assert_eq!(recovered_public_key, original.public_key);
        assert_eq!(recovered_session_port, 9000);
        assert_eq!(recovered_chunk_port, 9001);
        assert_eq!(recovered_contract, Contract::Bulk as u8);
        assert_eq!(recovered_version, 7);
        assert_eq!(recovered_service_count, 3);
        assert_eq!(recovered_service_index, 1);
    }

    #[test]
    fn service_hashes_are_deterministic() {
        let a = service_hash(b"summit.file_transfer");
        let b = service_hash(b"summit.file_transfer");
        let c = service_hash(b"summit.messaging");
        assert_eq!(a, b, "same input must produce same hash");
        assert_ne!(a, c, "different inputs must produce different hashes");
    }

    #[test]
    fn handshake_init_round_trip() {
        let original = HandshakeInit {
            nonce: [0x55; 16],
            service_hash: [0x44; 32],
            noise_msg: [0x33; 32],
        };
        let bytes = original.as_bytes();
        assert_eq!(bytes.len(), 80);
        let recovered = HandshakeInit::read_from_prefix(bytes).unwrap();
        assert_eq!(recovered.nonce, original.nonce);
        assert_eq!(recovered.service_hash, original.service_hash);
        assert_eq!(recovered.noise_msg, original.noise_msg);
    }

    #[test]
    fn handshake_response_round_trip() {
        let original = HandshakeResponse {
            nonce: [0x77; 16],
            noise_msg: [0x88; 96], // changed from 128
        };
        let bytes = original.as_bytes();
        assert_eq!(bytes.len(), 112); // changed from 144
        let recovered = HandshakeResponse::read_from_prefix(bytes).unwrap();
        assert_eq!(recovered.nonce, original.nonce);
        assert_eq!(recovered.noise_msg, original.noise_msg);
    }

    #[test]
    fn contract_round_trip() {
        assert_eq!(Contract::try_from(0x01).unwrap(), Contract::Realtime);
        assert_eq!(Contract::try_from(0x02).unwrap(), Contract::Bulk);
        assert_eq!(Contract::try_from(0x03).unwrap(), Contract::Background);
        assert!(Contract::try_from(0x00).is_err());
        assert!(Contract::try_from(0xff).is_err());
    }

    #[test]
    fn contract_to_u8() {
        assert_eq!(u8::from(Contract::Realtime), 0x01);
        assert_eq!(u8::from(Contract::Bulk), 0x02);
        assert_eq!(u8::from(Contract::Background), 0x03);
    }

    #[test]
    fn unknown_contract_error_message() {
        let err = Contract::try_from(0xAB).unwrap_err();
        assert!(err.to_string().contains("0xab"));
    }

    #[test]
    fn schema_id_raw_is_zeroed() {
        assert_eq!(SCHEMA_ID_RAW, [0u8; 32]);
    }
}
