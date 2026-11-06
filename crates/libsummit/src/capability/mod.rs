//! Capability registry — tracks nearby peers and what they offer.
//!
//! The registry is a concurrent map from capability_hash to PeerEntry,
//! populated by the multicast listener and read by the session layer.
//! Entries expire after PEER_TTL_SECS if not refreshed.

pub mod broadcast;
pub mod listener;
