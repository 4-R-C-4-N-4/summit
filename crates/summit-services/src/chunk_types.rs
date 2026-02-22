//! Chunk types — the atomic unit of transmission in Summit.

use bytes::Bytes;

/// A chunk ready to be sent.
#[derive(Debug, Clone)]
pub struct OutgoingChunk {
    pub type_tag: u16,
    pub schema_id: [u8; 32],
    pub payload: Bytes,
    /// Priority class from the originating service's contract.
    /// Bits 0-1: 0x01 Realtime, 0x02 Bulk, 0x03 Background.
    pub priority_flags: u8,
}

/// A chunk received and verified.
#[derive(Debug, Clone)]
pub struct IncomingChunk {
    pub content_hash: [u8; 32],
    pub type_tag: u16,
    pub schema_id: [u8; 32],
    pub payload: Bytes,
}
