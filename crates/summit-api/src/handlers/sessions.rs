//! /sessions handlers — session inspection and management.

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::Json;
use serde::Serialize;

use super::{parse_session_id, ApiState};

// ── /sessions/:id (DELETE) ────────────────────────────────────────────────────

#[derive(Serialize)]
pub struct SessionDropResponse {
    pub session_id: String,
    pub dropped: bool,
}

pub async fn handle_session_drop(
    State(state): State<ApiState>,
    Path(session_id): Path<String>,
) -> Result<Json<SessionDropResponse>, (StatusCode, String)> {
    let id = parse_session_id(&session_id)?;
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
    pub session_id: String,
    pub peer_addr: String,
    pub peer_pubkey: String,
    pub contract: String,
    pub chunk_port: u16,
    pub uptime_secs: u64,
    pub trust_level: String,
}

pub async fn handle_session_inspect(
    State(state): State<ApiState>,
    Path(session_id): Path<String>,
) -> Result<Json<SessionInspectResponse>, (StatusCode, String)> {
    let id = parse_session_id(&session_id)?;

    let session = state
        .sessions
        .get(&id)
        .ok_or((StatusCode::NOT_FOUND, "session not found".to_string()))?;

    let meta = &session.value().meta;
    let trust_level = state.trust.check(&meta.peer_pubkey);

    Ok(Json(SessionInspectResponse {
        session_id: hex::encode(meta.session_id),
        peer_addr: meta.peer_addr.to_string(),
        peer_pubkey: hex::encode(meta.peer_pubkey),
        contract: format!("{:?}", meta.primary_contract()),
        chunk_port: meta.chunk_port,
        uptime_secs: meta.established_at.elapsed().as_secs(),
        trust_level: format!("{:?}", trust_level),
    }))
}
