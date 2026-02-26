//! /trust handlers — trust management endpoints.

use axum::extract::State;
use axum::http::StatusCode;
use axum::Json;
use serde::{Deserialize, Serialize};

use super::{parse_pubkey, ApiState};

// ── /trust (GET) ──────────────────────────────────────────────────────────────

#[derive(Serialize)]
pub struct TrustListResponse {
    pub rules: Vec<TrustRule>,
}

#[derive(Serialize)]
pub struct TrustRule {
    pub public_key: String,
    pub level: String,
}

pub async fn handle_trust_list(State(state): State<ApiState>) -> Json<TrustListResponse> {
    let rules = state
        .trust
        .list()
        .into_iter()
        .map(|(pubkey, level)| TrustRule {
            public_key: hex::encode(pubkey),
            level: format!("{:?}", level),
        })
        .collect();

    Json(TrustListResponse { rules })
}

// ── /trust/add (POST) ─────────────────────────────────────────────────────────

#[derive(Deserialize)]
pub struct TrustAddRequest {
    pub public_key: String,
}

#[derive(Serialize)]
pub struct TrustAddResponse {
    pub public_key: String,
    pub flushed_chunks: usize,
}

pub async fn handle_trust_add(
    State(state): State<ApiState>,
    Json(req): Json<TrustAddRequest>,
) -> Result<Json<TrustAddResponse>, (StatusCode, String)> {
    let pubkey = parse_pubkey(&req.public_key)?;

    state.trust.trust(pubkey);

    let buffered = state.untrusted_buffer.flush(&pubkey);
    let flushed_chunks = buffered.len();

    // Replay buffered chunks through the service dispatcher
    for chunk in buffered {
        if let Err(e) = state.replay_tx.send((pubkey, chunk)) {
            tracing::warn!(error = %e, "failed to send buffered chunk for replay");
        }
    }

    Ok(Json(TrustAddResponse {
        public_key: req.public_key,
        flushed_chunks,
    }))
}

// ── /trust/block (POST) ──────────────────────────────────────────────────────

#[derive(Deserialize)]
pub struct TrustBlockRequest {
    pub public_key: String,
}

#[derive(Serialize)]
pub struct TrustBlockResponse {
    pub public_key: String,
}

pub async fn handle_trust_block(
    State(state): State<ApiState>,
    Json(req): Json<TrustBlockRequest>,
) -> Result<Json<TrustBlockResponse>, (StatusCode, String)> {
    let pubkey = parse_pubkey(&req.public_key)?;

    state.trust.block(pubkey);
    state.untrusted_buffer.clear(&pubkey);

    Ok(Json(TrustBlockResponse {
        public_key: req.public_key,
    }))
}

// ── /trust/pending (GET) ─────────────────────────────────────────────────────

#[derive(Serialize)]
pub struct TrustPendingResponse {
    pub peers: Vec<PendingPeer>,
}

#[derive(Serialize)]
pub struct PendingPeer {
    pub public_key: String,
    pub buffered_chunks: usize,
}

pub async fn handle_trust_pending(State(state): State<ApiState>) -> Json<TrustPendingResponse> {
    let peers = state
        .untrusted_buffer
        .peers()
        .into_iter()
        .map(|(pubkey, count)| PendingPeer {
            public_key: hex::encode(pubkey),
            buffered_chunks: count,
        })
        .collect();

    Json(TrustPendingResponse { peers })
}
