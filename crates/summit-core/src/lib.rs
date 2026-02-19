//! summit-core — shared types, wire format, and cryptographic primitives.
//! All other Summit crates depend on this one.

pub mod wire;
pub mod crypto;
pub mod message;

pub use message::{MessageChunk, MessageType, MessageContent};
