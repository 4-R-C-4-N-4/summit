//! Session management — tracks active Noise_XX sessions.

pub mod initiator;
pub mod listener;
mod state;

pub use state::HandshakeTracker;

use std::collections::HashMap;
use summit_core::wire::{service_hash, Contract, ServiceHash};
use summit_services::ServiceOnSession;

/// Build the default set of active services for a newly established session.
pub fn default_active_services() -> HashMap<ServiceHash, ServiceOnSession> {
    let mut m = HashMap::new();
    for name in [
        b"summit.file_transfer" as &[u8],
        b"summit.messaging",
        b"summit.compute",
    ] {
        m.insert(
            service_hash(name),
            ServiceOnSession {
                contract: Contract::Bulk,
                chunk_port: 0,
            },
        );
    }
    m
}
