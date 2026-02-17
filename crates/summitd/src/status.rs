//! HTTP status endpoint — exposes daemon state as JSON.

use axum::{Router, Json, extract::State};
use axum::routing::{get, post};
use serde::{Serialize, Deserialize};
use tokio::net::TcpListener;

use crate::session::SessionTable;
use crate::cache::ChunkCache;
use crate::capability::PeerRegistry;

#[derive(Clone)]
pub struct StatusState {
    pub sessions:    SessionTable,
    pub cache:       ChunkCache,
    pub registry:    PeerRegistry,
    pub chunk_tx:    tokio::sync::mpsc::UnboundedSender<crate::chunk::OutgoingChunk>,
    pub reassembler: std::sync::Arc<crate::transfer::FileReassembler>,
}

// ── /status ──────────────────────────────────────────────────────────────────

#[derive(Serialize)]
pub struct StatusResponse {
    pub sessions:         Vec<SessionInfo>,
    pub cache:            CacheInfo,
    pub peers_discovered: usize,
}

#[derive(Serialize)]
pub struct SessionInfo {
    pub session_id:       String,
    pub peer:             String,
    pub contract:         String,
    pub chunk_port:       u16,
    pub established_secs: u64,
}

#[derive(Serialize)]
pub struct CacheInfo {
    pub chunks: usize,
    pub bytes:  u64,
}

async fn handle_status(State(state): State<StatusState>) -> Json<StatusResponse> {
    let sessions = state.sessions.iter().map(|e| {
        let meta = &e.value().meta;
        SessionInfo {
            session_id:       hex::encode(meta.session_id),
                                             peer:             meta.peer_addr.to_string(),
                                             contract:         format!("{:?}", meta.contract),
                                             chunk_port:       meta.chunk_port,
                                             established_secs: meta.established_at.elapsed().as_secs(),
        }
    }).collect();

    let cache = CacheInfo {
        chunks: state.cache.count(),
        bytes:  state.cache.size(),
    };

    let peers_discovered = state.registry.len();

    Json(StatusResponse { sessions, cache, peers_discovered })
}

// ── /peers ────────────────────────────────────────────────────────────────────

#[derive(Serialize)]
pub struct PeersResponse {
    pub peers: Vec<PeerInfo>,
}

#[derive(Serialize)]
pub struct PeerInfo {
    pub public_key:   String,
    pub addr:         String,
    pub session_port: u16,
    pub chunk_port:   u16,
    pub contract:     u8,
    pub version:      u32,
    pub last_seen_secs: u64,
}

async fn handle_peers(State(state): State<StatusState>) -> Json<PeersResponse> {
    let peers = state.registry.iter().map(|e| {
        let p = e.value();
        PeerInfo {
            public_key:     hex::encode(p.public_key),
                addr:           p.addr.to_string(),
                                          session_port:   p.session_port,
                                          chunk_port:     p.chunk_port,
                                          contract:       p.contract,
                                          version:        p.version,
                                          last_seen_secs: p.last_seen.elapsed().as_secs(),
        }
    }).collect();

    Json(PeersResponse { peers })
}

// ── /cache ────────────────────────────────────────────────────────────────────

async fn handle_cache(State(state): State<StatusState>) -> Json<CacheInfo> {
    Json(CacheInfo {
        chunks: state.cache.count(),
         bytes:  state.cache.size(),
    })
}

// ── /cache/clear ──────────────────────────────────────────────────────────────

#[derive(Serialize, Deserialize)]
pub struct ClearResponse {
    pub cleared: usize,
}

async fn handle_cache_clear(State(state): State<StatusState>) -> Json<ClearResponse> {
    let cleared = state.cache.count();
    state.cache.clear();
    tracing::info!(cleared, "cache cleared via CLI");
    Json(ClearResponse { cleared })
}
// ── /send ─────────────────────────────────────────────────────────────────────

use axum::extract::Multipart;

#[derive(Serialize)]
pub struct SendResponse {
    pub filename:     String,
    pub bytes:        u64,
    pub chunks_sent:  usize,
}

async fn handle_send(
    State(state): State<StatusState>,
                     mut multipart: Multipart,
) -> Result<Json<SendResponse>, (axum::http::StatusCode, String)> {
    use crate::transfer;

    // Extract file from multipart
    let mut file_data = Vec::new();
    let mut filename = String::from("uploaded_file");

    while let Some(field) = multipart.next_field().await
        .map_err(|e| (axum::http::StatusCode::BAD_REQUEST, e.to_string()))?
        {
            if let Some(name) = field.file_name() {
                filename = name.to_string();
            }
            let data = field.bytes().await
            .map_err(|e| (axum::http::StatusCode::BAD_REQUEST, e.to_string()))?;
            file_data.extend_from_slice(&data);
        }

        if file_data.is_empty() {
            return Err((axum::http::StatusCode::BAD_REQUEST, "no file data".to_string()));
        }

        // Write to temp file
        let temp_path = std::env::temp_dir().join(&filename);
        std::fs::write(&temp_path, &file_data)
        .map_err(|e| (axum::http::StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

        // Chunk the file
        let chunks = transfer::chunk_file(&temp_path)
        .map_err(|e| (axum::http::StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

        let bytes = file_data.len() as u64;
        let chunks_sent = chunks.len();

        // Push all chunks to send queue
        for chunk in chunks {
            state.chunk_tx.send(chunk)
            .map_err(|_| (axum::http::StatusCode::INTERNAL_SERVER_ERROR, "send queue closed".to_string()))?;
        }

        tracing::info!(filename, bytes, chunks_sent, "file queued for sending");

        Ok(Json(SendResponse { filename, bytes, chunks_sent }))
}

// ── /files ────────────────────────────────────────────────────────────────────

#[derive(Serialize)]
pub struct FilesResponse {
    pub received:    Vec<String>,
    pub in_progress: Vec<String>,
}

async fn handle_files(State(state): State<StatusState>) -> Json<FilesResponse> {
    let received_dir = std::path::PathBuf::from("/tmp/summit-received");
    let mut received = Vec::new();

    if let Ok(entries) = std::fs::read_dir(&received_dir) {
        for entry in entries.flatten() {
            if let Some(name) = entry.file_name().to_str() {
                received.push(name.to_string());
            }
        }
    }

    let in_progress = state.reassembler.in_progress().await;

    Json(FilesResponse { received, in_progress })
}

// ── Router ────────────────────────────────────────────────────────────────────

pub async fn serve(state: StatusState, port: u16) -> anyhow::Result<()> {
    let app = Router::new()
    .route("/status",      get(handle_status))
    .route("/peers",       get(handle_peers))
    .route("/cache",       get(handle_cache))
    .route("/cache/clear", post(handle_cache_clear))
    .route("/send",        post(handle_send))
    .route("/files",       get(handle_files))
    .with_state(state);

    let listener = TcpListener::bind(format!("127.0.0.1:{}", port)).await?;
    tracing::info!(port, "status endpoint listening");
    axum::serve(listener, app).await?;
    Ok(())
}
