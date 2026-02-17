//! HTTP status endpoint — exposes daemon state as JSON.

use axum::{Router, Json, extract::State};
use axum::routing::get;
use serde::Serialize;
use tokio::net::TcpListener;

use crate::session::SessionTable;
use crate::cache::ChunkCache;
use crate::capability::PeerRegistry;

#[derive(Clone)]
pub struct StatusState {
    pub sessions: SessionTable,
    pub cache:    ChunkCache,
    pub registry: PeerRegistry,
}

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

pub async fn serve(state: StatusState, port: u16) -> anyhow::Result<()> {
    let app = Router::new()
        .route("/status", get(handle_status))
        .with_state(state);

    let listener = TcpListener::bind(format!("127.0.0.1:{}", port)).await?;
    tracing::info!(port, "status endpoint listening");
    axum::serve(listener, app).await?;
    Ok(())
}
