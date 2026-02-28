//! Session management — tracks active Noise_XX sessions.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Instant;

use dashmap::DashMap;
use tokio::net::UdpSocket;
use tokio::sync::Mutex;

use summit_core::crypto::Session;
use summit_core::wire::{Contract, ServiceHash};

use crate::qos::TokenBucket;

/// Per-service state within a session.
#[derive(Debug, Clone)]
pub struct ServiceOnSession {
    /// The contract governing this service's chunks.
    pub contract: Contract,
    /// Dedicated chunk port for this service. 0 = use session default.
    pub chunk_port: u16,
}

/// Metadata about an active session, stored alongside the crypto state.
#[derive(Debug)]
pub struct SessionMeta {
    /// Stable identifier — identical on both sides.
    pub session_id: [u8; 32],
    /// Peer's link-local address.
    pub peer_addr: std::net::SocketAddr,
    /// Default chunk port from handshake (session-level).
    pub chunk_port: u16,
    /// When this session was established.
    pub established_at: Instant,
    pub peer_pubkey: [u8; 32],

    /// Services active on this session, with their contracts.
    /// Built during post-handshake negotiation by intersecting
    /// local and remote service sets.
    pub active_services: HashMap<ServiceHash, ServiceOnSession>,
}

impl SessionMeta {
    /// Get the contract for a specific service on this session.
    pub fn contract_for(&self, service: &ServiceHash) -> Option<Contract> {
        self.active_services.get(service).map(|s| s.contract)
    }

    /// Is this service active on this session?
    pub fn has_service(&self, service: &ServiceHash) -> bool {
        self.active_services.contains_key(service)
    }

    /// Convenience: get a single contract if all services use the same one.
    /// Falls back to Bulk if mixed. Used during migration for code that
    /// still expects a single contract.
    pub fn primary_contract(&self) -> Contract {
        let mut contracts: Vec<_> = self.active_services.values().map(|s| s.contract).collect();
        contracts.dedup();
        if contracts.len() == 1 {
            contracts[0]
        } else {
            Contract::Bulk
        }
    }
}

/// An active session — crypto state, metadata, and dedicated I/O socket.
pub struct ActiveSession {
    pub meta: SessionMeta,
    pub crypto: Arc<Mutex<Session>>,
    pub socket: Arc<UdpSocket>, // Dedicated socket for chunk I/O
    pub bucket: Arc<Mutex<TokenBucket>>,
}

/// The session table — shared across all tasks.
pub type SessionTable = Arc<DashMap<[u8; 32], ActiveSession>>;

/// Create a new empty session table.
pub fn new_session_table() -> SessionTable {
    Arc::new(DashMap::new())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_session_table_creates_empty() {
        let table = new_session_table();
        assert!(table.is_empty());
        assert_eq!(table.len(), 0);
    }
}
