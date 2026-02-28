//! Recovery protocol — NACK-based chunk retransmission for Bulk transfers.
//!
//! When a receiver detects missing chunks (via the hash manifest in file
//! metadata), it sends a NACK listing the missing content hashes. The
//! sender looks them up in its ChunkCache and re-sends. If the cache has
//! been evicted, the sender responds with GONE so the receiver can give up.

use serde::{Deserialize, Serialize};

/// NACK payload — sent by the receiver to request retransmission.
///
/// Wire: schema_id = recovery_hash(), type_tag = recovery::NACK
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Nack {
    /// Content hashes the receiver is missing.
    pub missing: Vec<[u8; 32]>,

    /// Which NACK attempt this is (0 = first/targeted, 1+ = broadcast).
    pub attempt: u8,
}

/// Maximum hashes in a single NACK message.
/// With 32 bytes per hash + JSON overhead, 512 hashes fits in ~20KB
/// (well under MAX_PAYLOAD of 65535).
pub const MAX_NACK_HASHES: usize = 512;

/// Max consecutive NACK attempts with no progress before giving up.
pub const MAX_NACK_STALLS: u8 = 3;

/// Capacity advertisement — sent post-handshake so the sender tunes
/// its token bucket to the receiver's advertised rate.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Capacity {
    /// Tokens/sec the receiver can absorb.
    pub bulk_rate: u32,
    /// Burst capacity.
    pub bulk_burst: u32,
}

/// GONE payload — sent by the sender when requested chunks are no longer cached.
///
/// Wire: schema_id = recovery_hash(), type_tag = recovery::GONE
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Gone {
    /// Content hashes the sender no longer has.
    pub hashes: Vec<[u8; 32]>,
}
