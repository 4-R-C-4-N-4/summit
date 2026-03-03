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

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    use axum::extract::{Path, State};
    use axum::Json;

    fn test_state() -> ApiState {
        let (chunk_tx, chunk_rx) = tokio::sync::mpsc::channel(64);
        let (replay_tx, replay_rx) = tokio::sync::mpsc::unbounded_channel();
        let (shutdown_tx, _shutdown_rx) = tokio::sync::broadcast::channel(1);

        // Leak receivers so the senders remain valid for the test's duration.
        std::mem::forget(chunk_rx);
        std::mem::forget(replay_rx);

        let tmp = std::env::temp_dir().join(format!("summit-api-test-{}", std::process::id()));

        let cache = summit_services::ChunkCache::new(tmp.join("cache")).unwrap();

        let reassembler = Arc::new(summit_services::FileReassembler::new(tmp.join("files")));

        ApiState {
            sessions: summit_services::new_session_table(),
            cache,
            registry: summit_services::new_registry(),
            chunk_tx,
            reassembler,
            trust: summit_services::TrustRegistry::new(),
            untrusted_buffer: summit_services::UntrustedBuffer::new(),
            message_store: summit_services::MessageStore::new(),
            compute_store: summit_services::ComputeStore::new(),
            keypair: Arc::new(summit_core::crypto::Keypair::generate()),
            file_transfer_path: tmp.join("received"),
            enabled_services: vec!["messaging".into(), "compute".into()],
            replay_tx,
            shutdown_tx,
        }
    }

    // ── parse_pubkey tests ───────────────────────────────────────────────

    #[test]
    fn parse_pubkey_valid_64_hex() {
        let hex = "a".repeat(64);
        let result = parse_pubkey(&hex);
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), [0xAA; 32]);
    }

    #[test]
    fn parse_pubkey_invalid_hex() {
        let result = parse_pubkey("zzzz");
        assert!(result.is_err());
        let (status, _) = result.unwrap_err();
        assert_eq!(status, StatusCode::BAD_REQUEST);
    }

    #[test]
    fn parse_pubkey_wrong_length() {
        let hex = "aa".repeat(16); // 16 bytes, not 32
        let result = parse_pubkey(&hex);
        assert!(result.is_err());
        let (status, msg) = result.unwrap_err();
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert!(msg.contains("32 bytes"));
    }

    // ── parse_session_id tests ───────────────────────────────────────────

    #[test]
    fn parse_session_id_valid() {
        let hex = "bb".repeat(32);
        let result = parse_session_id(&hex);
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), [0xBB; 32]);
    }

    #[test]
    fn parse_session_id_invalid_hex() {
        let result = parse_session_id("not-hex!!");
        assert!(result.is_err());
    }

    #[test]
    fn parse_session_id_wrong_length() {
        let hex = "cc".repeat(10);
        let result = parse_session_id(&hex);
        assert!(result.is_err());
        let (_, msg) = result.unwrap_err();
        assert!(msg.contains("32 bytes"));
    }

    // ── compute handler tests ────────────────────────────────────────────

    #[tokio::test]
    async fn compute_all_tasks_empty() {
        let state = test_state();
        let Json(resp) = compute::handle_compute_all_tasks(State(state)).await;
        assert!(resp.tasks.is_empty());
    }

    #[tokio::test]
    async fn compute_all_tasks_with_tasks() {
        let state = test_state();
        let peer = [1u8; 32];
        state.compute_store.submit(
            peer,
            summit_services::TaskSubmit {
                task_id: "t1".to_string(),
                sender: "a".repeat(64),
                timestamp: 100,
                payload: serde_json::json!({}),
            },
        );
        let Json(resp) = compute::handle_compute_all_tasks(State(state)).await;
        assert_eq!(resp.tasks.len(), 1);
        assert_eq!(resp.tasks[0].task_id, "t1");
    }

    #[tokio::test]
    async fn compute_tasks_valid_peer() {
        let state = test_state();
        let peer = [0xAA; 32];
        state.compute_store.submit(
            peer,
            summit_services::TaskSubmit {
                task_id: "t2".to_string(),
                sender: "a".repeat(64),
                timestamp: 100,
                payload: serde_json::json!({}),
            },
        );
        let peer_hex = "aa".repeat(32);
        let Ok(Json(resp)) = compute::handle_compute_tasks(State(state), Path(peer_hex)).await
        else {
            panic!("expected Ok");
        };
        assert_eq!(resp.tasks.len(), 1);
    }

    #[tokio::test]
    async fn compute_tasks_invalid_hex() {
        let state = test_state();
        let result = compute::handle_compute_tasks(State(state), Path("zzzz".into())).await;
        let Err((status, _)) = result else {
            panic!("expected error");
        };
        assert_eq!(status, StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn compute_submit_valid() {
        let state = test_state();
        let peer_hex = "bb".repeat(32);
        let req = compute::ComputeSubmitRequest {
            to: peer_hex,
            payload: serde_json::json!({ "run": "echo hi" }),
        };
        let Ok(Json(resp)) = compute::handle_compute_submit(State(state.clone()), Json(req)).await
        else {
            panic!("expected Ok");
        };
        assert!(!resp.task_id.is_empty());

        // Task should be tracked in store
        let task = state.compute_store.get_task(&resp.task_id).unwrap();
        assert!(task.local);
    }

    // ── message handler tests ────────────────────────────────────────────

    #[tokio::test]
    async fn get_messages_valid_peer() {
        let state = test_state();
        let peer = [0xCC; 32];
        state.message_store.add(
            peer,
            summit_services::MessageEnvelope {
                msg_id: "m1".into(),
                msg_type: "text".into(),
                sender: "a".repeat(64),
                timestamp: 100,
                payload: serde_json::json!({ "text": "hi" }),
            },
        );
        let peer_hex = "cc".repeat(32);
        let Ok(Json(resp)) = messages::handle_get_messages(State(state), Path(peer_hex)).await
        else {
            panic!("expected Ok");
        };
        assert_eq!(resp.messages.len(), 1);
    }

    #[tokio::test]
    async fn get_messages_invalid_hex() {
        let state = test_state();
        let Err((status, _)) =
            messages::handle_get_messages(State(state), Path("nope".into())).await
        else {
            panic!("expected error");
        };
        assert_eq!(status, StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn send_message_valid() {
        let state = test_state();
        let peer_hex = "dd".repeat(32);
        let req = messages::SendMessageRequest {
            to: peer_hex,
            text: "hello world".into(),
        };
        let Ok(Json(resp)) = messages::handle_send_message(State(state.clone()), Json(req)).await
        else {
            panic!("expected Ok");
        };
        assert!(!resp.msg_id.is_empty());

        // Message should be stored
        let peer = [0xDD; 32];
        let msgs = state.message_store.get(&peer);
        assert_eq!(msgs.len(), 1);
    }

    // ── trust handler tests ──────────────────────────────────────────────

    #[tokio::test]
    async fn trust_list_empty() {
        let state = test_state();
        let Json(resp) = trust::handle_trust_list(State(state)).await;
        assert!(resp.rules.is_empty());
    }

    #[tokio::test]
    async fn trust_add_and_list() {
        let state = test_state();
        let peer_hex = "ee".repeat(32);
        let req = trust::TrustAddRequest {
            public_key: peer_hex.clone(),
        };
        let Ok(Json(resp)) = trust::handle_trust_add(State(state.clone()), Json(req)).await else {
            panic!("expected Ok");
        };
        assert_eq!(resp.flushed_chunks, 0);

        let Json(list) = trust::handle_trust_list(State(state)).await;
        assert_eq!(list.rules.len(), 1);
        assert_eq!(list.rules[0].public_key, peer_hex);
    }

    #[tokio::test]
    async fn trust_block_clears_buffer() {
        let state = test_state();
        let peer = [0xFF; 32];
        let peer_hex = "ff".repeat(32);

        // Buffer some chunks
        state.untrusted_buffer.add(
            peer,
            [0u8; 32],
            0,
            [0u8; 32],
            bytes::Bytes::from_static(b"data"),
        );
        assert_eq!(state.untrusted_buffer.count(&peer), 1);

        let req = trust::TrustBlockRequest {
            public_key: peer_hex,
        };
        assert!(trust::handle_trust_block(State(state.clone()), Json(req))
            .await
            .is_ok());

        // Buffer should be cleared
        assert_eq!(state.untrusted_buffer.count(&peer), 0);
    }

    #[tokio::test]
    async fn trust_pending_shows_buffered_peers() {
        let state = test_state();
        let peer = [0x11; 32];
        state.untrusted_buffer.add(
            peer,
            [0u8; 32],
            0,
            [0u8; 32],
            bytes::Bytes::from_static(b"x"),
        );
        let Json(resp) = trust::handle_trust_pending(State(state)).await;
        assert_eq!(resp.peers.len(), 1);
        assert_eq!(resp.peers[0].buffered_chunks, 1);
    }

    // ── session handler tests ────────────────────────────────────────────

    #[tokio::test]
    async fn session_drop_invalid_id() {
        let state = test_state();
        match sessions::handle_session_drop(State(state), Path("bad".into())).await {
            Err((status, _)) => assert_eq!(status, StatusCode::BAD_REQUEST),
            Ok(_) => panic!("expected error"),
        }
    }

    #[tokio::test]
    async fn session_inspect_nonexistent() {
        let state = test_state();
        let id_hex = "00".repeat(32);
        match sessions::handle_session_inspect(State(state), Path(id_hex)).await {
            Err((status, _)) => assert_eq!(status, StatusCode::NOT_FOUND),
            Ok(_) => panic!("expected error"),
        }
    }

    // ── status handler tests ─────────────────────────────────────────────

    #[tokio::test]
    async fn cache_returns_count_and_size() {
        let state = test_state();
        let Json(resp) = status::handle_cache(State(state)).await;
        assert_eq!(resp.chunks, 0);
        assert_eq!(resp.bytes, 0);
    }

    #[tokio::test]
    async fn cache_clear_returns_cleared() {
        let state = test_state();
        let Json(resp) = status::handle_cache_clear(State(state)).await;
        assert_eq!(resp.cleared, 0);
    }

    #[tokio::test]
    async fn services_returns_list_with_enabled() {
        let state = test_state();
        let Json(resp) = status::handle_services(State(state)).await;
        assert_eq!(resp.services.len(), 4);

        let messaging = resp
            .services
            .iter()
            .find(|s| s.name == "messaging")
            .unwrap();
        assert!(messaging.enabled);

        let compute = resp.services.iter().find(|s| s.name == "compute").unwrap();
        assert!(compute.enabled);

        let stream = resp
            .services
            .iter()
            .find(|s| s.name == "stream_udp")
            .unwrap();
        assert!(!stream.enabled);
    }

    #[tokio::test]
    async fn schema_list_returns_five() {
        let Json(resp) = status::handle_schema_list().await;
        assert_eq!(resp.schemas.len(), 5);
    }
}
