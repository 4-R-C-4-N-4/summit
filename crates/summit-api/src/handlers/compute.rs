//! /compute handlers — remote compute task endpoints.

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::Json;
use serde::{Deserialize, Serialize};

use summit_services::{ComputeEnvelope, OutgoingChunk, SendTarget, TaskSubmit};

use super::{parse_pubkey, ApiState};

// ── /compute/tasks (GET) ──────────────────────────────────────────────────────

#[derive(Serialize)]
pub struct ComputeAllTasksResponse {
    pub tasks: Vec<ComputeTaskJson>,
}

pub async fn handle_compute_all_tasks(
    State(state): State<ApiState>,
) -> Json<ComputeAllTasksResponse> {
    let tasks = state
        .compute_store
        .all_tasks()
        .into_iter()
        .map(task_to_json)
        .collect();

    Json(ComputeAllTasksResponse { tasks })
}

// ── /compute/tasks/{peer_pubkey} (GET) ────────────────────────────────────────

#[derive(Serialize)]
pub struct ComputeTasksResponse {
    pub peer_pubkey: String,
    pub tasks: Vec<ComputeTaskJson>,
}

#[derive(Serialize)]
pub struct ComputeTaskJson {
    pub task_id: String,
    pub status: String,
    pub submitted_at: u64,
    pub updated_at: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub elapsed_ms: Option<u64>,
    pub payload: serde_json::Value,
}

pub async fn handle_compute_tasks(
    State(state): State<ApiState>,
    Path(peer_pubkey): Path<String>,
) -> Result<Json<ComputeTasksResponse>, (StatusCode, String)> {
    let pubkey = parse_pubkey(&peer_pubkey)?;

    let task_ids = state.compute_store.tasks_for_peer(&pubkey);
    let tasks = task_ids
        .iter()
        .filter_map(|id| state.compute_store.get_task(id))
        .map(task_to_json)
        .collect();

    Ok(Json(ComputeTasksResponse { peer_pubkey, tasks }))
}

// ── /compute/submit (POST) ────────────────────────────────────────────────────

#[derive(Deserialize)]
pub struct ComputeSubmitRequest {
    pub to: String,
    pub payload: serde_json::Value,
}

#[derive(Serialize)]
pub struct ComputeSubmitResponse {
    pub task_id: String,
    pub timestamp: u64,
}

pub async fn handle_compute_submit(
    State(state): State<ApiState>,
    Json(req): Json<ComputeSubmitRequest>,
) -> Result<Json<ComputeSubmitResponse>, (StatusCode, String)> {
    let to = parse_pubkey(&req.to)?;
    let from = state.keypair.public;

    let timestamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis() as u64;

    let payload_bytes = serde_json::to_vec(&req.payload)
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    let task_id = {
        let mut h = blake3::Hasher::new();
        h.update(&from);
        h.update(&timestamp.to_le_bytes());
        h.update(&payload_bytes);
        hex::encode(h.finalize().as_bytes())
    };

    let submit = TaskSubmit {
        task_id: task_id.clone(),
        sender: hex::encode(from),
        timestamp,
        payload: req.payload,
    };

    let envelope = ComputeEnvelope {
        msg_type: summit_services::compute_types::msg_types::TASK_SUBMIT.to_string(),
        payload: serde_json::to_value(&submit)
            .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?,
    };

    let raw = serde_json::to_vec(&envelope)
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    let chunk = OutgoingChunk {
        type_tag: 0,
        schema_id: summit_core::wire::compute_hash(),
        payload: bytes::Bytes::from(raw),
        priority_flags: 0x02,
    };

    let target = SendTarget::Peer { public_key: to };
    state.chunk_tx.send((target, chunk)).await.map_err(|_| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            "send queue closed".to_string(),
        )
    })?;

    state.compute_store.track_submitted(to, submit);

    tracing::info!(
        task_id = &task_id[..16],
        to = &req.to[..16.min(req.to.len())],
        "compute task submitted"
    );

    Ok(Json(ComputeSubmitResponse { task_id, timestamp }))
}

// ── Helpers ───────────────────────────────────────────────────────────────────

fn task_to_json(t: summit_services::ComputeTask) -> ComputeTaskJson {
    let (result, elapsed_ms) = match &t.result {
        Some(r) => (Some(r.result.clone()), Some(r.elapsed_ms)),
        None => (None, None),
    };
    ComputeTaskJson {
        task_id: t.submit.task_id.clone(),
        status: format!("{:?}", t.status),
        submitted_at: t.submitted_at,
        updated_at: t.updated_at,
        result,
        elapsed_ms,
        payload: t.submit.payload.clone(),
    }
}
