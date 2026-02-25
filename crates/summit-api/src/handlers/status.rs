//! /status, /peers, /cache, /services, /schema, /daemon/shutdown handlers.

use axum::extract::State;
use axum::Json;
use serde::Serialize;

use summit_services::KnownSchema;

use super::ApiState;

// ── /status ──────────────────────────────────────────────────────────────────

#[derive(Serialize)]
pub struct StatusResponse {
    pub sessions: Vec<SessionInfo>,
    pub cache: CacheInfo,
    pub peers_discovered: usize,
}

#[derive(Serialize)]
pub struct SessionInfo {
    pub session_id: String,
    pub peer: String,
    pub peer_pubkey: String,
    pub contract: String,
    pub chunk_port: u16,
    pub established_secs: u64,
    pub trust_level: String,
}

#[derive(Serialize)]
pub struct CacheInfo {
    pub chunks: usize,
    pub bytes: u64,
}

pub async fn handle_status(State(state): State<ApiState>) -> Json<StatusResponse> {
    let sessions = state
        .sessions
        .iter()
        .map(|e| {
            let meta = &e.value().meta;
            let trust_level = state.trust.check(&meta.peer_pubkey);
            SessionInfo {
                session_id: hex::encode(meta.session_id),
                peer: meta.peer_addr.to_string(),
                peer_pubkey: hex::encode(meta.peer_pubkey),
                contract: format!("{:?}", meta.primary_contract()),
                chunk_port: meta.chunk_port,
                established_secs: meta.established_at.elapsed().as_secs(),
                trust_level: format!("{:?}", trust_level),
            }
        })
        .collect();

    let cache = CacheInfo {
        chunks: state.cache.count(),
        bytes: state.cache.size(),
    };

    let peers_discovered = state.registry.len();

    Json(StatusResponse {
        sessions,
        cache,
        peers_discovered,
    })
}

// ── /peers ────────────────────────────────────────────────────────────────────

#[derive(Serialize)]
pub struct PeersResponse {
    pub peers: Vec<PeerInfo>,
}

#[derive(Serialize)]
pub struct PeerInfo {
    pub public_key: String,
    pub addr: String,
    pub session_port: u16,
    pub services: Vec<String>,
    pub service_count: usize,
    pub is_complete: bool,
    pub version: u32,
    pub last_seen_secs: u64,
    pub trust_level: String,
    pub buffered_chunks: usize,
}

pub async fn handle_peers(State(state): State<ApiState>) -> Json<PeersResponse> {
    let peers = state
        .registry
        .iter()
        .map(|e| {
            let p = e.value();
            let pubkey = *e.key();
            let trust_level = state.trust.check(&pubkey);
            let buffered_chunks = state.untrusted_buffer.count(&pubkey);
            let services: Vec<String> = p.services.keys().map(hex::encode).collect();

            PeerInfo {
                public_key: hex::encode(p.public_key),
                addr: p.addr.to_string(),
                session_port: p.session_port,
                services,
                service_count: p.expected_service_count as usize,
                is_complete: p.is_complete(),
                version: p.version,
                last_seen_secs: p.last_seen.elapsed().as_secs(),
                trust_level: format!("{:?}", trust_level),
                buffered_chunks,
            }
        })
        .collect();

    Json(PeersResponse { peers })
}

// ── /cache ────────────────────────────────────────────────────────────────────

pub async fn handle_cache(State(state): State<ApiState>) -> Json<CacheInfo> {
    Json(CacheInfo {
        chunks: state.cache.count(),
        bytes: state.cache.size(),
    })
}

#[derive(Serialize)]
pub struct ClearResponse {
    pub cleared: usize,
}

pub async fn handle_cache_clear(State(state): State<ApiState>) -> Json<ClearResponse> {
    let cleared = state.cache.count();
    state.cache.clear();
    tracing::info!(cleared, "cache cleared via CLI");
    Json(ClearResponse { cleared })
}

// ── /schema ───────────────────────────────────────────────────────────────────

#[derive(Serialize)]
pub struct SchemaListResponse {
    pub schemas: Vec<SchemaInfoItem>,
}

#[derive(Serialize)]
pub struct SchemaInfoItem {
    pub id: String,
    pub name: String,
    pub type_tag: u8,
}

pub async fn handle_schema_list() -> Json<SchemaListResponse> {
    let schemas = vec![
        SchemaInfoItem {
            id: hex::encode(KnownSchema::TestPing.id()),
            name: KnownSchema::TestPing.name().to_string(),
            type_tag: 1,
        },
        SchemaInfoItem {
            id: hex::encode(KnownSchema::FileData.id()),
            name: KnownSchema::FileData.name().to_string(),
            type_tag: 2,
        },
        SchemaInfoItem {
            id: hex::encode(KnownSchema::FileMetadata.id()),
            name: KnownSchema::FileMetadata.name().to_string(),
            type_tag: 3,
        },
        SchemaInfoItem {
            id: hex::encode(KnownSchema::Message.id()),
            name: KnownSchema::Message.name().to_string(),
            type_tag: 4,
        },
        SchemaInfoItem {
            id: hex::encode(KnownSchema::ComputeTask.id()),
            name: KnownSchema::ComputeTask.name().to_string(),
            type_tag: 5,
        },
    ];

    Json(SchemaListResponse { schemas })
}

// ── /services ─────────────────────────────────────────────────────────────────

#[derive(Serialize)]
pub struct ServicesResponse {
    pub services: Vec<ServiceStatus>,
}

#[derive(Serialize)]
pub struct ServiceStatus {
    pub name: String,
    pub enabled: bool,
    pub contract: String,
}

pub async fn handle_services(State(state): State<ApiState>) -> Json<ServicesResponse> {
    let all = [
        ("file_transfer", "Bulk"),
        ("messaging", "Bulk"),
        ("stream_udp", "Realtime"),
        ("compute", "Bulk"),
    ];

    let services = all
        .iter()
        .map(|(name, contract)| ServiceStatus {
            name: name.to_string(),
            enabled: state.enabled_services.iter().any(|s| s == name),
            contract: contract.to_string(),
        })
        .collect();

    Json(ServicesResponse { services })
}

// ── /daemon/shutdown ──────────────────────────────────────────────────────────

#[derive(Serialize)]
pub struct ShutdownResponse {
    pub message: String,
}

pub async fn handle_shutdown() -> Json<ShutdownResponse> {
    tracing::info!("shutdown requested via API");

    tokio::spawn(async {
        tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
        std::process::exit(0);
    });

    Json(ShutdownResponse {
        message: "Shutdown initiated".to_string(),
    })
}
