//! Messaging service — receives JSON message chunks and stores them.

use crate::message_store::MessageStore;
use crate::service::ChunkService;
use summit_core::message::{message_schema_id, MessageEnvelope};
use summit_core::wire::{ChunkHeader, Contract, ServiceHash};

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
        message_schema_id()
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

        self.store.add_from_envelope(peer_pubkey, &envelope)?;

        Ok(())
    }
}
