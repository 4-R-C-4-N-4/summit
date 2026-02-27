//! summit-core — shared types, wire format, and cryptographic primitives.
//! All other Summit crates depend on this one.
#![allow(clippy::derivable_impls)]

pub mod config;
pub mod crypto;
pub mod recovery;
pub mod wire;
