//! /messages handlers — messaging endpoints.

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::Json;
use serde::{Deserialize, Serialize};

use summit_services::{messaging_schema_id, MessageEnvelope, OutgoingChunk, SendTarget};

use super::{parse_pubkey, ApiState};

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

pub async fn handle_get_messages(
    State(state): State<ApiState>,
    Path(peer_pubkey): Path<String>,
) -> Result<Json<MessagesResponse>, (StatusCode, String)> {
    let pubkey = parse_pubkey(&peer_pubkey)?;

    let messages = state.message_store.get(&pubkey);

    let messages_json: Vec<MessageJson> = messages
        .into_iter()
        .map(|m| MessageJson {
            msg_id: m.msg_id,
            from: m.sender,
            to: peer_pubkey.clone(),
            msg_type: m.msg_type,
            timestamp: m.timestamp,
            content: m.payload,
        })
        .collect();

    Ok(Json(MessagesResponse {
        peer_pubkey,
        messages: messages_json,
    }))
}

// ── /messages/send (POST) ─────────────────────────────────────────────────────

#[derive(Deserialize)]
pub struct SendMessageRequest {
    pub to: String,
    pub text: String,
}

#[derive(Serialize)]
pub struct SendMessageResponse {
    pub msg_id: String,
    pub timestamp: u64,
}

pub async fn handle_send_message(
    State(state): State<ApiState>,
    Json(req): Json<SendMessageRequest>,
) -> Result<Json<SendMessageResponse>, (StatusCode, String)> {
    let to = parse_pubkey(&req.to)?;
    let from = state.keypair.public;

    let timestamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis() as u64;

    let payload_value = serde_json::json!({ "text": req.text });

    let payload_bytes = serde_json::to_vec(&payload_value).unwrap();
    let msg_id = {
        let mut h = blake3::Hasher::new();
        h.update(&from);
        h.update(&timestamp.to_le_bytes());
        h.update(&payload_bytes);
        hex::encode(h.finalize().as_bytes())
    };

    let envelope = MessageEnvelope {
        msg_id: msg_id.clone(),
        msg_type: summit_services::msg_types::TEXT.to_string(),
        sender: hex::encode(from),
        timestamp,
        payload: payload_value,
    };

    let raw = serde_json::to_vec(&envelope)
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    let chunk = OutgoingChunk {
        type_tag: 0,
        schema_id: messaging_schema_id(),
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

    state.message_store.add(to, envelope);

    Ok(Json(SendMessageResponse { msg_id, timestamp }))
}
