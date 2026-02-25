//! Chunk transmission — the data plane.
//!
//! Chunks are the atomic unit of transmission in Summit. Every chunk
//! has a header (content hash, schema ID, type tag) and a payload.
//! The chunk layer handles encryption, verification, and caching.

pub mod manager;
pub mod receive;
pub mod send;
pub mod send_worker;

// Re-export from summit-services for convenience within summitd
pub use summit_services::{IncomingChunk, OutgoingChunk};
