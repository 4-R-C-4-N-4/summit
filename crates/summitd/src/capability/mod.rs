//! Capability registry — tracks nearby peers and what they offer.
//!
//! The registry is a concurrent map from capability_hash to PeerEntry,
//! populated by the multicast listener and read by the session layer.
//! Entries expire after PEER_TTL_SECS if not refreshed.

use std::net::Ipv6Addr;
use std::sync::Arc;
use std::time::Instant;

use dashmap::DashMap;
use summit_core::wire::Contract;

pub mod broadcast;
pub mod listener;

// ── Registry ──────────────────────────────────────────────────────────────────

/// A peer offering a capability, as seen in a capability announcement.
#[derive(Debug, Clone)]
pub struct PeerEntry {
    /// The peer's link-local IPv6 address — source of the announcement datagram.
    pub addr: Ipv6Addr,
    /// The peer's static public key — used to verify identity during handshake.
    pub public_key: [u8; 32],
    /// UDP port on which the peer accepts session handshake initiation.
    pub session_port: u16,
    /// Capability version — prefer the highest seen for a given capability_hash.
    pub version: u32,
    /// Latency contract this capability operates under.
    pub contract: Contract,
    /// When this entry was last refreshed. Used for TTL expiry.
    pub last_seen: Instant,
}

/// The peer registry — shared between broadcast, listener, and session tasks.
///
/// Keyed on capability_hash ([u8; 32]).
/// DashMap gives lock-free reads — multiple tasks can query concurrently.
pub type PeerRegistry = Arc<DashMap<[u8; 32], PeerEntry>>;

/// Create a new empty peer registry.
pub fn new_registry() -> PeerRegistry {
    Arc::new(DashMap::new())
}
