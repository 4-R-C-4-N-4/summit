//! Chunk transmission — the data plane.
//!
//! Chunks are the atomic unit of transmission in Summit. Every chunk
//! has a header (content hash, schema ID, type tag) and a payload.
//! The chunk layer handles encryption, verification, and caching.

pub mod receive;
pub mod send;

// Re-export from summit-services for convenience within summitd
pub use summit_services::{IncomingChunk, OutgoingChunk};
