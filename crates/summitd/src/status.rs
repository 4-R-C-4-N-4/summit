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
    pub sessions: SessionTable,
    pub cache:    ChunkCache,
    pub registry: PeerRegistry,
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

// ── Router ────────────────────────────────────────────────────────────────────

pub async fn serve(state: StatusState, port: u16) -> anyhow::Result<()> {
    let app = Router::new()
    .route("/status",      get(handle_status))
    .route("/peers",       get(handle_peers))
    .route("/cache",       get(handle_cache))
    .route("/cache/clear", post(handle_cache_clear))
    .with_state(state);

    let listener = TcpListener::bind(format!("127.0.0.1:{}", port)).await?;
    tracing::info!(port, "status endpoint listening");
    axum::serve(listener, app).await?;
    Ok(())
}
