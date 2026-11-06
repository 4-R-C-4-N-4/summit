//! Session management — tracks active Noise_XX sessions.

pub mod initiator;
pub mod listener;
mod state;

pub use state::HandshakeTracker;

use std::collections::HashMap;
use std::time::Duration;
use summit_core::wire::{service_hash, Contract, ServiceHash};
use summit_services::ServiceOnSession;

/// Normally only the peer with the lower public key initiates. If no session
/// exists after this long, the higher-key peer initiates too. This covers
/// one-way multicast: the lower-key peer can't initiate to a peer whose
/// announcements never reach it (e.g. a phone without a MulticastLock).
pub const FALLBACK_INITIATE_AFTER: Duration = Duration::from_secs(7);

/// Is `ours` the designated initiator towards `theirs`?
pub fn is_designated_initiator(ours: &[u8; 32], theirs: &[u8; 32]) -> bool {
    ours < theirs
}

/// Both sides sent HandshakeInit at once (the fallback fired while the
/// designated side was also trying). The designated initiator's attempt wins;
/// the other side abandons its own attempt and answers.
pub fn should_yield_to_incoming(ours: &[u8; 32], theirs: &[u8; 32]) -> bool {
    !is_designated_initiator(ours, theirs)
}

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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exactly_one_side_is_designated() {
        let (a, b) = ([1u8; 32], [2u8; 32]);
        assert!(is_designated_initiator(&a, &b));
        assert!(!is_designated_initiator(&b, &a));
    }

    #[test]
    fn simultaneous_init_resolves_to_one_handshake() {
        let (low, high) = ([1u8; 32], [2u8; 32]);
        // Low ignores high's Init; high yields to low's Init.
        assert!(!should_yield_to_incoming(&low, &high));
        assert!(should_yield_to_incoming(&high, &low));
    }
}
