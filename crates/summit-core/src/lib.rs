//! summit-core — shared types, wire format, and cryptographic primitives.
//! All other Summit crates depend on this one.

pub mod crypto;
pub mod message;
pub mod wire;

pub use message::{MessageChunk, MessageContent, MessageType};
