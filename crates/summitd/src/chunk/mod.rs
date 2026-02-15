//! Chunk transmission — the data plane.
//!
//! Chunks are the atomic unit of transmission in Summit. Every chunk
//! has a header (content hash, schema ID, type tag) and a payload.
//! The chunk layer handles encryption, verification, and caching.

use bytes::Bytes;

pub mod send;
pub mod receive;

/// A chunk ready to be sent.
#[derive(Debug, Clone)]
pub struct OutgoingChunk {
    pub type_tag:  u16,
    pub schema_id: [u8; 32],
    pub payload:   Bytes,
}

/// A chunk received and verified.
#[derive(Debug, Clone)]
pub struct IncomingChunk {
    pub content_hash: [u8; 32],
    pub type_tag:     u16,
    pub schema_id:    [u8; 32],
    pub payload:      Bytes,
}
