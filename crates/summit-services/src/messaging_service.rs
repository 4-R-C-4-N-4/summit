//! Messaging service — receives JSON message chunks and stores them.
//!
//! `MessageEnvelope` is the JSON wire format for all messaging chunks.
//! It is defined here because this service is the sole parser of that
//! format on the wire; `summit-core` has no opinion about chunk payloads.

use crate::message_store::MessageStore;
use crate::service::ChunkService;
use serde::{Deserialize, Serialize};
use summit_core::wire::{service_hash, ChunkHeader, Contract, ServiceHash};

// ── Wire format ───────────────────────────────────────────────────────────────

/// JSON envelope — the payload of every messaging chunk.
///
/// Senders populate `msg_id` as `hex(blake3(sender_bytes || timestamp_le ||
/// payload_bytes))` for deduplication. Receivers store unknown `msg_type`
/// values verbatim without processing them.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MessageEnvelope {
    /// Deduplication id: `hex(blake3(sender_bytes || timestamp_le || payload_bytes))`.
    pub msg_id: String,
    /// Well-known type string (see [`msg_types`]). Extensible.
    pub msg_type: String,
    /// Sender public key, hex-encoded.
    pub sender: String,
    /// Unix timestamp in milliseconds.
    pub timestamp: u64,
    /// Type-specific content. Structure is defined by `msg_type`.
    pub payload: serde_json::Value,
}

/// Well-known `msg_type` strings.
pub mod msg_types {
    pub const TEXT: &str = "text";
    pub const ACK: &str = "ack";
    pub const READ: &str = "read";
}

/// Schema identifier for messaging chunks (used in `ChunkHeader.schema_id`).
pub fn messaging_schema_id() -> ServiceHash {
    service_hash(b"summit.messaging")
}

// ── Service ───────────────────────────────────────────────────────────────────

pub struct MessagingService {
    store: MessageStore,
}

impl MessagingService {
    pub fn new(store: MessageStore) -> Self {
        Self { store }
    }
}

impl ChunkService for MessagingService {
    fn service_hash(&self) -> ServiceHash {
        messaging_schema_id()
    }

    fn contract(&self) -> Contract {
        Contract::Bulk
    }

    fn on_activate(&self, peer_pubkey: &[u8; 32]) {
        tracing::info!(
            peer = hex::encode(&peer_pubkey[..8]),
            "messaging service activated"
        );
    }

    fn on_deactivate(&self, peer_pubkey: &[u8; 32]) {
        tracing::debug!(
            peer = hex::encode(&peer_pubkey[..8]),
            "messaging service deactivated"
        );
    }

    fn handle_chunk(
        &self,
        peer_pubkey: &[u8; 32],
        _header: &ChunkHeader,
        payload: &[u8],
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let envelope: MessageEnvelope =
            serde_json::from_slice(payload).map_err(|e| format!("invalid message JSON: {e}"))?;

        tracing::debug!(
            sender = &envelope.sender[..16.min(envelope.sender.len())],
            msg_type = &envelope.msg_type,
            "message received"
        );

        self.store.add(*peer_pubkey, envelope);

        Ok(())
    }
}
