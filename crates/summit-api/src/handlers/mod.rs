//! HTTP API handlers — exposes daemon state as JSON.

pub mod compute;
pub mod files;
pub mod messages;
pub mod sessions;
pub mod status;
pub mod trust;

use std::sync::Arc;

use axum::http::StatusCode;

use summit_core::crypto::Keypair;
use summit_services::{
    BufferedChunk, ChunkCache, ComputeStore, MessageStore, OutgoingChunk, PeerRegistry, SendTarget,
    SessionTable, TrustRegistry, UntrustedBuffer,
};

#[derive(Clone)]
pub struct ApiState {
    pub sessions: SessionTable,
    pub cache: ChunkCache,
    pub registry: PeerRegistry,
    pub chunk_tx: tokio::sync::mpsc::Sender<(SendTarget, OutgoingChunk)>,
    pub reassembler: Arc<summit_services::FileReassembler>,
    pub trust: TrustRegistry,
    pub untrusted_buffer: UntrustedBuffer,
    pub message_store: MessageStore,
    pub compute_store: ComputeStore,
    pub keypair: Arc<Keypair>,
    /// Directory where received files are written.
    pub file_transfer_path: std::path::PathBuf,
    /// Names of services enabled in the current config, e.g. "messaging", "compute".
    pub enabled_services: Vec<String>,
    /// Channel to replay buffered chunks when a peer becomes trusted.
    pub replay_tx: tokio::sync::mpsc::UnboundedSender<([u8; 32], BufferedChunk)>,
    /// Shutdown broadcast sender — signals graceful daemon shutdown.
    pub shutdown_tx: tokio::sync::broadcast::Sender<()>,
}

// ── Shared helpers ────────────────────────────────────────────────────────────

/// Parse a hex-encoded 32-byte public key.
fn parse_pubkey(hex_str: &str) -> Result<[u8; 32], (StatusCode, String)> {
    let bytes =
        hex::decode(hex_str).map_err(|_| (StatusCode::BAD_REQUEST, "invalid hex".to_string()))?;
    if bytes.len() != 32 {
        return Err((
            StatusCode::BAD_REQUEST,
            "public key must be 32 bytes".to_string(),
        ));
    }
    let mut arr = [0u8; 32];
    arr.copy_from_slice(&bytes);
    Ok(arr)
}

/// Parse a hex-encoded 32-byte session ID.
fn parse_session_id(hex_str: &str) -> Result<[u8; 32], (StatusCode, String)> {
    let bytes =
        hex::decode(hex_str).map_err(|_| (StatusCode::BAD_REQUEST, "invalid hex".to_string()))?;
    if bytes.len() != 32 {
        return Err((
            StatusCode::BAD_REQUEST,
            "session_id must be 32 bytes".to_string(),
        ));
    }
    let mut arr = [0u8; 32];
    arr.copy_from_slice(&bytes);
    Ok(arr)
}

// Re-export handler functions for use in router setup.
pub use compute::{handle_compute_all_tasks, handle_compute_submit, handle_compute_tasks};
pub use files::{handle_files, handle_send};
pub use messages::{handle_get_messages, handle_send_message};
pub use sessions::{handle_session_drop, handle_session_inspect};
pub use status::{
    handle_cache, handle_cache_clear, handle_peers, handle_schema_list, handle_services,
    handle_shutdown, handle_status,
};
pub use trust::{handle_trust_add, handle_trust_block, handle_trust_list, handle_trust_pending};
