//! Session management — tracks active Noise_XX sessions.

use std::sync::Arc;
use std::time::Instant;

use dashmap::DashMap;
use tokio::sync::Mutex;
use tokio::net::UdpSocket;

use summit_core::crypto::Session;
use summit_core::wire::Contract;

pub mod handshake;

// ── Session Table ─────────────────────────────────────────────────────────────

/// Metadata about an active session, stored alongside the crypto state.
#[derive(Debug)]
pub struct SessionMeta {
    /// Stable identifier — identical on both sides.
    pub session_id: [u8; 32],
    /// Peer's link-local address.
    pub peer_addr: std::net::SocketAddr,
    /// Latency contract negotiated for this session.
    pub contract: Contract,
    /// When this session was established.
    pub established_at: Instant,
}

/// An active session — crypto state, metadata, and dedicated I/O socket.
pub struct ActiveSession {
    pub meta:   SessionMeta,
    pub crypto: Arc<Mutex<Session>>,
    pub socket: Arc<UdpSocket>,  // Dedicated socket for chunk I/O
}

/// The session table — shared across all tasks.
pub type SessionTable = Arc<DashMap<[u8; 32], ActiveSession>>;

/// Create a new empty session table.
pub fn new_session_table() -> SessionTable {
    Arc::new(DashMap::new())
}
