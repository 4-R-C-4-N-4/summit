//! Compute service — receives compute task chunks and stores them.
//!
//! Actual task execution is future work. This service handles receiving,
//! storing, and acknowledging tasks via the chunk transport.

use crate::compute_store::ComputeStore;
use crate::compute_types::{msg_types, ComputeEnvelope, TaskStatus, TaskSubmit};
use crate::service::ChunkService;
use summit_core::config::ComputeSettings;
use summit_core::wire::{ChunkHeader, Contract, ServiceHash};

pub struct ComputeService {
    store: ComputeStore,
    #[allow(dead_code)]
    settings: ComputeSettings,
}

impl ComputeService {
    pub fn new(store: ComputeStore, settings: ComputeSettings) -> Self {
        Self { store, settings }
    }
}

impl ChunkService for ComputeService {
    fn service_hash(&self) -> ServiceHash {
        summit_core::wire::compute_hash()
    }

    fn contract(&self) -> Contract {
        Contract::Bulk
    }

    fn on_activate(&self, peer_pubkey: &[u8; 32]) {
        tracing::info!(
            peer = hex::encode(&peer_pubkey[..8]),
            "compute service activated"
        );
    }

    fn on_deactivate(&self, peer_pubkey: &[u8; 32]) {
        tracing::debug!(
            peer = hex::encode(&peer_pubkey[..8]),
            "compute service deactivated"
        );
    }

    fn handle_chunk(
        &self,
        peer_pubkey: &[u8; 32],
        _header: &ChunkHeader,
        payload: &[u8],
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let envelope: ComputeEnvelope =
            serde_json::from_slice(payload).map_err(|e| format!("invalid compute JSON: {e}"))?;

        match envelope.msg_type.as_str() {
            msg_types::TASK_SUBMIT => {
                let submit: TaskSubmit = serde_json::from_value(envelope.payload)
                    .map_err(|e| format!("invalid task_submit payload: {e}"))?;
                tracing::debug!(
                    task_id = &submit.task_id[..16.min(submit.task_id.len())],
                    "compute task_submit received"
                );
                self.store.submit(*peer_pubkey, submit);
            }
            msg_types::TASK_CANCEL => {
                let task_id = envelope
                    .payload
                    .get("task_id")
                    .and_then(|v| v.as_str())
                    .ok_or("task_cancel missing task_id")?;
                tracing::debug!(
                    task_id = &task_id[..16.min(task_id.len())],
                    "compute task_cancel received"
                );
                self.store.update_status(task_id, TaskStatus::Cancelled);
            }
            other => {
                tracing::warn!(msg_type = other, "compute: unknown msg_type, ignoring");
            }
        }

        Ok(())
    }
}
