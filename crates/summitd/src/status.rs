//! HTTP status endpoint — exposes daemon state as JSON.

use axum::{Router, Json, extract::State};
use axum::routing::{get, post};
use axum::extract::Path;
use axum::http::StatusCode;
use serde::{Serialize, Deserialize};
use tokio::net::TcpListener;

use crate::session::SessionTable;
use crate::cache::ChunkCache;
use crate::capability::PeerRegistry;
use crate::trust::{TrustRegistry, TrustLevel, UntrustedBuffer};
use crate::send_target::SendTarget;
use crate::schema::KnownSchema;
use crate::message_store::MessageStore;

use summit_core::message::MessageChunk;
use tower_http::cors::{CorsLayer, Any};
use axum::response::{Response, IntoResponse};
use axum::body::Body;
use axum::http::{header, StatusCode as HttpStatusCode};


#[cfg(feature = "embed-ui")]
#[derive(RustEmbed)]
#[folder = "../../astral/dist"]
struct WebAssets;

#[derive(Clone)]
pub struct StatusState {
    pub sessions:         SessionTable,
    pub cache:            ChunkCache,
    pub registry:         PeerRegistry,
    pub chunk_tx:         tokio::sync::mpsc::UnboundedSender<(SendTarget, crate::chunk::OutgoingChunk)>,
    pub reassembler:      std::sync::Arc<crate::transfer::FileReassembler>,
    pub trust:            TrustRegistry,
    pub untrusted_buffer: UntrustedBuffer,
    pub message_store: MessageStore,
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
    pub peer_pubkey:      String,
    pub contract:         String,
    pub chunk_port:       u16,
    pub established_secs: u64,
    pub trust_level:      String,
}

#[derive(Serialize)]
pub struct CacheInfo {
    pub chunks: usize,
    pub bytes:  u64,
}

async fn handle_status(State(state): State<StatusState>) -> Json<StatusResponse> {
    let sessions = state.sessions.iter().map(|e| {
        let meta = &e.value().meta;
        let trust_level = state.trust.check(&meta.peer_pubkey);
        SessionInfo {
            session_id:       hex::encode(meta.session_id),
            peer:             meta.peer_addr.to_string(),
            peer_pubkey:      hex::encode(meta.peer_pubkey),
            contract:         format!("{:?}", meta.contract),
            chunk_port:       meta.chunk_port,
            established_secs: meta.established_at.elapsed().as_secs(),
            trust_level:      format!("{:?}", trust_level),
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
    pub public_key:      String,
    pub addr:            String,
    pub session_port:    u16,
    pub chunk_port:      u16,
    pub contract:        u8,
    pub version:         u32,
    pub last_seen_secs:  u64,
    pub trust_level:     String,
    pub buffered_chunks: usize,
}

async fn handle_peers(State(state): State<StatusState>) -> Json<PeersResponse> {
    let peers = state.registry.iter().map(|e| {
        let p = e.value();
        let pubkey = *e.key();
        let trust_level = state.trust.check(&pubkey);
        let buffered_chunks = state.untrusted_buffer.count(&pubkey);

        PeerInfo {
            public_key:      hex::encode(p.public_key),
                addr:            p.addr.to_string(),
                                          session_port:    p.session_port,
                                          chunk_port:      p.chunk_port,
                                          contract:        p.contract,
                                          version:         p.version,
                                          last_seen_secs:  p.last_seen.elapsed().as_secs(),
                                          trust_level:     format!("{:?}", trust_level),
                                          buffered_chunks,
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
    pub filename:    String,
    pub bytes:       u64,
    pub chunks_sent: usize,
}

async fn handle_send(
    State(state): State<StatusState>,
                     mut multipart: Multipart,
) -> Result<Json<SendResponse>, (axum::http::StatusCode, String)> {
    use crate::transfer;

    // Extract file and optional target from multipart
    let mut file_data = Vec::new();
    let mut filename = String::from("uploaded_file");
    let mut target = SendTarget::Broadcast;

    while let Some(field) = multipart.next_field().await
        .map_err(|e| (axum::http::StatusCode::BAD_REQUEST, e.to_string()))?
        {
            let field_name = field.name().unwrap_or("").to_string();

            if field_name == "target" {
                let target_str = field.text().await
                .map_err(|e| (axum::http::StatusCode::BAD_REQUEST, e.to_string()))?;
                target = serde_json::from_str(&target_str)
                .map_err(|e| (axum::http::StatusCode::BAD_REQUEST, e.to_string()))?;
            } else {
                if let Some(name) = field.file_name() {
                    filename = name.to_string();
                }
                let data = field.bytes().await
                .map_err(|e| (axum::http::StatusCode::BAD_REQUEST, e.to_string()))?;
                file_data.extend_from_slice(&data);
            }
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

        // Push all chunks to send queue with target
        for chunk in chunks {
            state.chunk_tx.send((target.clone(), chunk))
            .map_err(|_| (axum::http::StatusCode::INTERNAL_SERVER_ERROR, "send queue closed".to_string()))?;
        }

        tracing::info!(filename, bytes, chunks_sent, ?target, "file queued for sending");

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


// ── /message ────────────────────────────────────────────────────────────────────


// ── /messages/{peer_pubkey} (GET) ─────────────────────────────────────────────

#[derive(Serialize)]
pub struct MessagesResponse {
    pub peer_pubkey: String,
    pub messages: Vec<MessageJson>,
}

#[derive(Serialize)]
pub struct MessageJson {
    pub msg_id: String,
    pub from: String,
    pub to: String,
    pub msg_type: String,
    pub timestamp: u64,
    pub content: serde_json::Value,
}

async fn handle_get_messages(
    State(state): State<StatusState>,
                             Path(peer_pubkey): Path<String>,
) -> Result<Json<MessagesResponse>, (StatusCode, String)> {
    let pubkey_bytes = hex::decode(&peer_pubkey)
    .map_err(|_| (StatusCode::BAD_REQUEST, "invalid hex".to_string()))?;

    if pubkey_bytes.len() != 32 {
        return Err((StatusCode::BAD_REQUEST, "public key must be 32 bytes".to_string()));
    }

    let mut pubkey = [0u8; 32];
    pubkey.copy_from_slice(&pubkey_bytes);

    let messages = state.message_store.get(&pubkey);

    let messages_json: Vec<MessageJson> = messages.into_iter().map(|m| {
        MessageJson {
            msg_id: hex::encode(m.msg_id),
            from: hex::encode(m.sender),
            to: hex::encode(m.recipient),
            msg_type: format!("{:?}", m.msg_type),
            timestamp: m.timestamp,
            content: serde_json::to_value(&m.content).unwrap(),
        }
    }).collect();

    Ok(Json(MessagesResponse {
        peer_pubkey,
        messages: messages_json,
    }))
}

// ── /messages/send (POST) ─────────────────────────────────────────────────────

#[derive(Deserialize)]
pub struct SendMessageRequest {
    pub to: String,           // peer public key (hex)
    pub text: String,         // message text
}

#[derive(Serialize)]
pub struct SendMessageResponse {
    pub msg_id: String,
    pub timestamp: u64,
}

async fn handle_send_message(
    State(state): State<StatusState>,
                             Json(req): Json<SendMessageRequest>,
) -> Result<Json<SendMessageResponse>, (StatusCode, String)> {
    // Parse recipient public key
    let to_bytes = hex::decode(&req.to)
    .map_err(|_| (StatusCode::BAD_REQUEST, "invalid recipient pubkey".to_string()))?;

    if to_bytes.len() != 32 {
        return Err((StatusCode::BAD_REQUEST, "public key must be 32 bytes".to_string()));
    }

    let mut to = [0u8; 32];
    to.copy_from_slice(&to_bytes);

    // TODO: Get our own public key from keypair
    let from = [0u8; 32];  // Replace with actual keypair.public

    // Create message
    let message = MessageChunk::text(from, to, req.text);

    // Serialize to chunk payload
    let payload = message.to_bytes();

    // Create outgoing chunk with type_tag 4 (message)
    let chunk = crate::chunk::OutgoingChunk {
        type_tag: 4,
        schema_id: KnownSchema::Message.id(),
        payload: bytes::Bytes::from(payload),
    };

    // Send to peer
    let target = crate::send_target::SendTarget::Peer { public_key: to };
    state.chunk_tx.send((target, chunk))
    .map_err(|_| (StatusCode::INTERNAL_SERVER_ERROR, "send queue closed".to_string()))?;

    // Store locally
    state.message_store.add(to, message.clone());

    Ok(Json(SendMessageResponse {
        msg_id: hex::encode(message.msg_id),
            timestamp: message.timestamp,
    }))
}


// ── /trust ────────────────────────────────────────────────────────────────────

#[derive(Serialize)]
pub struct TrustListResponse {
    pub rules: Vec<TrustRule>,
}

#[derive(Serialize)]
pub struct TrustRule {
    pub public_key: String,
    pub level:      String,
}

async fn handle_trust_list(State(state): State<StatusState>) -> Json<TrustListResponse> {
    let rules = state.trust.list().into_iter().map(|(pubkey, level)| {
        TrustRule {
            public_key: hex::encode(pubkey),
            level:      format!("{:?}", level),
        }
    }).collect();

    Json(TrustListResponse { rules })
}

#[derive(Deserialize)]
pub struct TrustAddRequest {
    pub public_key: String,
}

#[derive(Serialize)]
pub struct TrustAddResponse {
    pub public_key:     String,
    pub flushed_chunks: usize,
}

async fn handle_trust_add(
    State(state): State<StatusState>,
                          Json(req): Json<TrustAddRequest>,
) -> Result<Json<TrustAddResponse>, (axum::http::StatusCode, String)> {
    let pubkey_bytes = hex::decode(&req.public_key)
    .map_err(|_| (axum::http::StatusCode::BAD_REQUEST, "invalid hex".to_string()))?;

    if pubkey_bytes.len() != 32 {
        return Err((axum::http::StatusCode::BAD_REQUEST, "public key must be 32 bytes".to_string()));
    }

    let mut pubkey = [0u8; 32];
    pubkey.copy_from_slice(&pubkey_bytes);

    // Mark as trusted
    state.trust.trust(pubkey);

    // Flush buffered chunks
    let buffered = state.untrusted_buffer.flush(&pubkey);
    let flushed_chunks = buffered.len();

    // TODO: Process flushed chunks through reassembler

    Ok(Json(TrustAddResponse {
        public_key: req.public_key,
            flushed_chunks,
    }))
}

#[derive(Deserialize)]
pub struct TrustBlockRequest {
    pub public_key: String,
}

#[derive(Serialize)]
pub struct TrustBlockResponse {
    pub public_key: String,
}

async fn handle_trust_block(
    State(state): State<StatusState>,
                            Json(req): Json<TrustBlockRequest>,
) -> Result<Json<TrustBlockResponse>, (axum::http::StatusCode, String)> {
    let pubkey_bytes = hex::decode(&req.public_key)
    .map_err(|_| (axum::http::StatusCode::BAD_REQUEST, "invalid hex".to_string()))?;

    if pubkey_bytes.len() != 32 {
        return Err((axum::http::StatusCode::BAD_REQUEST, "public key must be 32 bytes".to_string()));
    }

    let mut pubkey = [0u8; 32];
    pubkey.copy_from_slice(&pubkey_bytes);

    state.trust.block(pubkey);
    state.untrusted_buffer.clear(&pubkey);

    Ok(Json(TrustBlockResponse {
        public_key: req.public_key,
    }))
}

#[derive(Serialize)]
pub struct TrustPendingResponse {
    pub peers: Vec<PendingPeer>,
}

#[derive(Serialize)]
pub struct PendingPeer {
    pub public_key:      String,
    pub buffered_chunks: usize,
}

async fn handle_trust_pending(State(state): State<StatusState>) -> Json<TrustPendingResponse> {
    let peers = state.untrusted_buffer.peers().into_iter().map(|(pubkey, count)| {
        PendingPeer {
            public_key:      hex::encode(pubkey),
                buffered_chunks: count,
        }
    }).collect();

    Json(TrustPendingResponse { peers })
}

// ── /daemon/shutdown ──────────────────────────────────────────────────────────

#[derive(Serialize)]
pub struct ShutdownResponse {
    pub message: String,
}

async fn handle_shutdown() -> Json<ShutdownResponse> {
    tracing::info!("shutdown requested via API");

    // Spawn a task to exit after responding
    tokio::spawn(async {
        tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
        std::process::exit(0);
    });

    Json(ShutdownResponse {
        message: "Shutdown initiated".to_string(),
    })
}

// ── /sessions/:id (DELETE) ────────────────────────────────────────────────────

#[derive(Serialize)]
pub struct SessionDropResponse {
    pub session_id: String,
    pub dropped:    bool,
}

async fn handle_session_drop(
    State(state): State<StatusState>,
                             Path(session_id): Path<String>,
) -> Result<Json<SessionDropResponse>, (StatusCode, String)> {
    let id_bytes = hex::decode(&session_id)
    .map_err(|_| (StatusCode::BAD_REQUEST, "invalid hex".to_string()))?;

    if id_bytes.len() != 32 {
        return Err((StatusCode::BAD_REQUEST, "session_id must be 32 bytes".to_string()));
    }

    let mut id = [0u8; 32];
    id.copy_from_slice(&id_bytes);

    let dropped = state.sessions.remove(&id).is_some();

    if dropped {
        tracing::info!(session_id = %session_id, "session dropped via API");
    }

    Ok(Json(SessionDropResponse {
        session_id,
        dropped,
    }))
}

// ── /sessions/:id (GET) ───────────────────────────────────────────────────────

#[derive(Serialize)]
pub struct SessionInspectResponse {
    pub session_id:   String,
    pub peer_addr:    String,
    pub peer_pubkey:  String,
    pub contract:     String,
    pub chunk_port:   u16,
    pub uptime_secs:  u64,
    pub trust_level:  String,
}

async fn handle_session_inspect(
    State(state): State<StatusState>,
                                Path(session_id): Path<String>,
) -> Result<Json<SessionInspectResponse>, (StatusCode, String)> {
    let id_bytes = hex::decode(&session_id)
    .map_err(|_| (StatusCode::BAD_REQUEST, "invalid hex".to_string()))?;

    if id_bytes.len() != 32 {
        return Err((StatusCode::BAD_REQUEST, "session_id must be 32 bytes".to_string()));
    }

    let mut id = [0u8; 32];
    id.copy_from_slice(&id_bytes);

    let session = state.sessions.get(&id)
    .ok_or((StatusCode::NOT_FOUND, "session not found".to_string()))?;

    let meta = &session.value().meta;
    let trust_level = state.trust.check(&meta.peer_pubkey);

    Ok(Json(SessionInspectResponse {
        session_id:   hex::encode(meta.session_id),
            peer_addr:    meta.peer_addr.to_string(),
            peer_pubkey:  hex::encode(meta.peer_pubkey),
            contract:     format!("{:?}", meta.contract),
            chunk_port:   meta.chunk_port,
            uptime_secs:  meta.established_at.elapsed().as_secs(),
            trust_level:  format!("{:?}", trust_level),
    }))
}

// ── /schema ───────────────────────────────────────────────────────────────────

#[derive(Serialize)]
pub struct SchemaListResponse {
    pub schemas: Vec<SchemaInfoItem>,
}

#[derive(Serialize)]
pub struct SchemaInfoItem {
    pub id:       String,
    pub name:     String,
    pub type_tag: u8,
}

async fn handle_schema_list() -> Json<SchemaListResponse> {
    let schemas = vec![
        SchemaInfoItem {
            id:       hex::encode(KnownSchema::TestPing.id()),
            name:     KnownSchema::TestPing.name().to_string(),
            type_tag: 1,
        },
        SchemaInfoItem {
            id:       hex::encode(KnownSchema::FileData.id()),
            name:     KnownSchema::FileData.name().to_string(),
            type_tag: 2,
        },
        SchemaInfoItem {
            id:       hex::encode(KnownSchema::FileMetadata.id()),
            name:     KnownSchema::FileMetadata.name().to_string(),
            type_tag: 3,
        },
    ];

    Json(SchemaListResponse { schemas })
}

// ── Static Files (SPA) ────────────────────────────────────────────────────────


#[cfg(feature = "embed-ui")]
async fn serve_static(uri: axum::http::Uri) -> impl IntoResponse {
    let path = uri.path().trim_start_matches('/');

    let file_path = if path.is_empty() || !path.contains('.') {
        "index.html"
    } else {
        path
    };

    match WebAssets::get(file_path) {
        Some(content) => {
            let mime = mime_guess::from_path(file_path).first_or_octet_stream();
            Response::builder()
            .status(HttpStatusCode::OK)
            .header(header::CONTENT_TYPE, mime.as_ref())
            .body(Body::from(content.data))
            .unwrap()
        }
        None => {
            if let Some(index) = WebAssets::get("index.html") {
                Response::builder()
                .status(HttpStatusCode::OK)
                .header(header::CONTENT_TYPE, "text/html")
                .body(Body::from(index.data))
                .unwrap()
            } else {
                Response::builder()
                .status(HttpStatusCode::NOT_FOUND)
                .body(Body::from("Not found"))
                .unwrap()
            }
        }
    }
}

#[cfg(not(feature = "embed-ui"))]
async fn serve_static(_uri: axum::http::Uri) -> impl IntoResponse {
    Response::builder()
    .status(HttpStatusCode::OK)
    .header(header::CONTENT_TYPE, "text/html")
    .body(Body::from(
        "<!DOCTYPE html>
        <html>
        <head><title>Summit API</title></head>
        <body>
        <h1>Summit API Server</h1>
        <p>API available at <code>/api/*</code></p>
        <p>Build with UI: <code>cargo build --features embed-ui</code></p>
        </body>
        </html>"
    ))
    .unwrap()
}

// ── Router ────────────────────────────────────────────────────────────────────

pub async fn serve(state: StatusState, port: u16) -> anyhow::Result<()> {
    let cors = CorsLayer::new()
    .allow_origin(Any)
    .allow_methods(Any)
    .allow_headers(Any);
    // for prod:
    //.allow_origin("http://localhost:{the-app-port}".parse::<HeaderValue>().unwrap())

    let api_routes = Router::new()
    .route("/status",           get(handle_status))
    .route("/peers",            get(handle_peers))
    .route("/cache",            get(handle_cache))
    .route("/cache/clear",      post(handle_cache_clear))
    .route("/send",             post(handle_send))
    .route("/files",            get(handle_files))
    .route("/trust",            get(handle_trust_list))
    .route("/trust/add",        post(handle_trust_add))
    .route("/trust/block",      post(handle_trust_block))
    .route("/trust/pending",    get(handle_trust_pending))
    .route("/daemon/shutdown",  post(handle_shutdown))
    .route("/sessions/{id}",     axum::routing::delete(handle_session_drop))
    .route("/sessions/{id}",     get(handle_session_inspect))
    .route("/schema",           get(handle_schema_list))
    .route("/messages/:peer_pubkey", get(handle_get_messages))
    .route("/messages/send", post(handle_send_message))
    .with_state(state);


    // Combine API and static files
    let app = Router::new()
    .nest("/api", api_routes)
    .fallback(serve_static)  // Catch-all for static files and SPA routing
    .layer(cors);

    let listener = TcpListener::bind(format!("0.0.0.0:{}", port)).await?;
    tracing::info!(port, "status endpoint listening");
    axum::serve(listener, app).await?;
    Ok(())
}
