//! Capability registry — tracks nearby peers and what they offer.

use std::collections::HashMap;
use std::net::Ipv6Addr;
use std::sync::Arc;
use std::time::Instant;

use dashmap::DashMap;
use summit_core::wire::{Contract, ServiceHash};

/// Tracked state for a discovered peer.
///
/// Accumulates service announcements over multiple datagrams.
/// A peer is fully discovered when `services.len() >= expected_service_count`.
#[derive(Debug, Clone)]
pub struct PeerEntry {
    /// Peer's link-local address (from the UDP source address).
    pub addr: Ipv6Addr,

    /// Ed25519 public key.
    pub public_key: [u8; 32],

    /// TCP port for session handshakes.
    pub session_port: u16,

    /// Protocol version.
    pub version: u32,

    /// Services this peer offers.
    /// Key: service hash. Value: (contract, chunk_port).
    pub services: HashMap<ServiceHash, (Contract, u16)>,

    /// How many services the peer says it offers (from service_count field).
    pub expected_service_count: u8,

    /// Last time any datagram arrived from this peer.
    pub last_seen: Instant,
}

impl PeerEntry {
    /// Create from the first announcement datagram seen for this peer.
    pub fn from_first_announcement(
        addr: Ipv6Addr,
        ann: &summit_core::wire::CapabilityAnnouncement,
    ) -> Self {
        let contract = Contract::try_from(ann.contract).unwrap_or(Contract::Bulk);
        let mut services = HashMap::new();
        services.insert(ann.service_hash, (contract, ann.chunk_port));

        Self {
            addr,
            public_key: ann.public_key,
            session_port: ann.session_port,
            version: ann.version,
            services,
            expected_service_count: ann.service_count,
            last_seen: Instant::now(),
        }
    }

    /// Update from a subsequent announcement datagram.
    pub fn update_from_announcement(
        &mut self,
        ann: &summit_core::wire::CapabilityAnnouncement,
    ) {
        let contract = Contract::try_from(ann.contract).unwrap_or(Contract::Bulk);
        self.services.insert(ann.service_hash, (contract, ann.chunk_port));
        self.session_port = ann.session_port;
        self.expected_service_count = ann.service_count;
        self.last_seen = Instant::now();
    }

    /// Have we received all announced services?
    pub fn is_complete(&self) -> bool {
        self.services.len() >= self.expected_service_count as usize
    }

    /// Does this peer offer a specific service?
    pub fn has_service(&self, hash: &ServiceHash) -> bool {
        self.services.contains_key(hash)
    }

    /// Get the contract for a specific service on this peer.
    pub fn service_contract(&self, hash: &ServiceHash) -> Option<Contract> {
        self.services.get(hash).map(|(c, _)| *c)
    }

    /// Get the chunk port for a specific service on this peer.
    pub fn service_chunk_port(&self, hash: &ServiceHash) -> Option<u16> {
        self.services.get(hash).map(|(_, p)| *p)
    }
}

/// The peer registry — shared between broadcast, listener, and session tasks.
/// Keyed on public key.
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
