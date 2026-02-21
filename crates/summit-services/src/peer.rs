//! Capability registry — tracks nearby peers and what they offer.

use std::net::Ipv6Addr;
use std::sync::Arc;
use std::time::Instant;

use dashmap::DashMap;

/// A peer offering a capability, as seen in a capability announcement.
#[derive(Debug, Clone)]
pub struct PeerEntry {
    /// The peer's link-local IPv6 address — source of the announcement datagram.
    pub addr: Ipv6Addr,
    /// The peer's static public key — used to verify identity during handshake.
    pub public_key: [u8; 32],
    /// UDP port on which the peer accepts session handshake initiation.
    pub session_port: u16,
    /// Store chunk port
    pub chunk_port: u16,
    /// Capability version — prefer the highest seen for a given capability_hash.
    pub version: u32,
    /// Latency contract this capability operates under.
    pub contract: u8,
    /// When this entry was last refreshed. Used for TTL expiry.
    pub last_seen: Instant,
}

/// The peer registry — shared between broadcast, listener, and session tasks.
///
/// Keyed on PUBLIC KEY ([u8; 32]) — unique per peer, unlike capability_hash
/// which is shared by all peers advertising the same capability.
pub type PeerRegistry = Arc<DashMap<[u8; 32], PeerEntry>>;

/// Create a new empty peer registry.
pub fn new_registry() -> PeerRegistry {
    Arc::new(DashMap::new())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_registry_creates_empty() {
        let registry = new_registry();
        assert!(registry.is_empty());
        assert_eq!(registry.len(), 0);
    }
}
