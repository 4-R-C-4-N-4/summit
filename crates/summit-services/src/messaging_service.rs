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
    ) -> anyhow::Result<()> {
        let envelope: MessageEnvelope = serde_json::from_slice(payload)
            .map_err(|e| anyhow::anyhow!("invalid message JSON: {e}"))?;

        tracing::debug!(
            sender = &envelope.sender[..16.min(envelope.sender.len())],
            msg_type = &envelope.msg_type,
            "message received"
        );

        self.store.add(*peer_pubkey, envelope);

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::service::ChunkService;

    fn make_service() -> MessagingService {
        MessagingService::new(MessageStore::new())
    }

    fn dummy_header() -> summit_core::wire::ChunkHeader {
        summit_core::wire::ChunkHeader {
            content_hash: [0u8; 32],
            schema_id: messaging_schema_id(),
            type_tag: 0,
            length: 0,
            flags: 0,
            version: 1,
        }
    }

    fn make_envelope(msg_id: &str, timestamp: u64) -> MessageEnvelope {
        MessageEnvelope {
            msg_id: msg_id.to_string(),
            msg_type: msg_types::TEXT.to_string(),
            sender: "a".repeat(64),
            timestamp,
            payload: serde_json::json!({ "text": "hello" }),
        }
    }

    #[test]
    fn handle_chunk_valid_message() {
        let svc = make_service();
        let peer = [1u8; 32];
        let env = make_envelope("msg-1", 100);
        let payload = serde_json::to_vec(&env).unwrap();

        svc.handle_chunk(&peer, &dummy_header(), &payload).unwrap();

        let msgs = svc.store.get(&peer);
        assert_eq!(msgs.len(), 1);
        assert_eq!(msgs[0].msg_id, "msg-1");
    }

    #[test]
    fn handle_chunk_invalid_json() {
        let svc = make_service();
        let peer = [1u8; 32];

        let result = svc.handle_chunk(&peer, &dummy_header(), b"garbage");
        assert!(result.is_err());
    }

    #[test]
    fn handle_chunk_multiple_from_same_peer() {
        let svc = make_service();
        let peer = [1u8; 32];

        for i in 0..3 {
            let env = make_envelope(&format!("msg-{i}"), 100 + i);
            let payload = serde_json::to_vec(&env).unwrap();
            svc.handle_chunk(&peer, &dummy_header(), &payload).unwrap();
        }

        let msgs = svc.store.get(&peer);
        assert_eq!(msgs.len(), 3);
        assert_eq!(msgs[0].msg_id, "msg-0");
        assert_eq!(msgs[1].msg_id, "msg-1");
        assert_eq!(msgs[2].msg_id, "msg-2");
    }

    #[test]
    fn handle_chunk_different_peers() {
        let svc = make_service();
        let peer_a = [1u8; 32];
        let peer_b = [2u8; 32];

        let env_a = make_envelope("msg-a", 100);
        let env_b = make_envelope("msg-b", 200);

        svc.handle_chunk(
            &peer_a,
            &dummy_header(),
            &serde_json::to_vec(&env_a).unwrap(),
        )
        .unwrap();
        svc.handle_chunk(
            &peer_b,
            &dummy_header(),
            &serde_json::to_vec(&env_b).unwrap(),
        )
        .unwrap();

        assert_eq!(svc.store.get(&peer_a).len(), 1);
        assert_eq!(svc.store.get(&peer_b).len(), 1);
        assert_eq!(svc.store.get(&peer_a)[0].msg_id, "msg-a");
        assert_eq!(svc.store.get(&peer_b)[0].msg_id, "msg-b");
    }

    #[test]
    fn service_hash_matches_schema_id() {
        let svc = make_service();
        assert_eq!(svc.service_hash(), messaging_schema_id());
    }

    #[test]
    fn contract_is_bulk() {
        let svc = make_service();
        assert_eq!(svc.contract(), Contract::Bulk);
    }
}
