//! Compute service — receives compute task chunks and stores them.
//!
//! When a `task_submit` arrives the service stores it, then sends a
//! `task_ack` back to the submitter so they can see the task was received.

use crate::chunk_types::OutgoingChunk;
use crate::compute_store::ComputeStore;
use crate::compute_types::{msg_types, ComputeEnvelope, TaskAck, TaskStatus, TaskSubmit};
use crate::send_target::SendTarget;
use crate::service::ChunkService;
use summit_core::config::ComputeSettings;
use summit_core::wire::{ChunkHeader, Contract, ServiceHash};
use tokio::sync::mpsc;

pub struct ComputeService {
    store: ComputeStore,
    #[allow(dead_code)]
    settings: ComputeSettings,
    chunk_tx: mpsc::Sender<(SendTarget, OutgoingChunk)>,
}

impl ComputeService {
    pub fn new(
        store: ComputeStore,
        settings: ComputeSettings,
        chunk_tx: mpsc::Sender<(SendTarget, OutgoingChunk)>,
    ) -> Self {
        Self {
            store,
            settings,
            chunk_tx,
        }
    }

    /// Build and send a `task_ack` chunk back to the submitting peer.
    fn send_ack(&self, peer_pubkey: &[u8; 32], task_id: &str, status: TaskStatus) {
        let ack = TaskAck {
            task_id: task_id.to_string(),
            status,
        };
        let envelope = ComputeEnvelope {
            msg_type: msg_types::TASK_ACK.to_string(),
            payload: match serde_json::to_value(&ack) {
                Ok(v) => v,
                Err(e) => {
                    tracing::warn!(error = %e, "failed to serialize task_ack");
                    return;
                }
            },
        };
        let raw = match serde_json::to_vec(&envelope) {
            Ok(v) => v,
            Err(e) => {
                tracing::warn!(error = %e, "failed to encode task_ack envelope");
                return;
            }
        };
        let chunk = OutgoingChunk {
            type_tag: 0,
            schema_id: summit_core::wire::compute_hash(),
            payload: bytes::Bytes::from(raw),
            priority_flags: 0x02,
        };
        let target = SendTarget::Peer {
            public_key: *peer_pubkey,
        };
        if let Err(e) = self.chunk_tx.try_send((target, chunk)) {
            tracing::warn!(error = %e, "failed to enqueue task_ack");
        }
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
                tracing::info!(
                    task_id = &submit.task_id[..16.min(submit.task_id.len())],
                    peer = hex::encode(&peer_pubkey[..8]),
                    "compute task_submit received"
                );
                let task_id = submit.task_id.clone();
                self.store.submit(*peer_pubkey, submit);
                // ACK back to the submitter so their status updates
                self.send_ack(peer_pubkey, &task_id, TaskStatus::Queued);
            }
            msg_types::TASK_ACK => {
                let ack: TaskAck = serde_json::from_value(envelope.payload)
                    .map_err(|e| format!("invalid task_ack payload: {e}"))?;
                tracing::info!(
                    task_id = &ack.task_id[..16.min(ack.task_id.len())],
                    status = ?ack.status,
                    "compute task_ack received"
                );
                self.store.ack(&ack.task_id, ack.status);
            }
            msg_types::TASK_RESULT => {
                let result: crate::compute_types::TaskResult =
                    serde_json::from_value(envelope.payload)
                        .map_err(|e| format!("invalid task_result payload: {e}"))?;
                tracing::info!(
                    task_id = &result.task_id[..16.min(result.task_id.len())],
                    elapsed_ms = result.elapsed_ms,
                    "compute task_result received"
                );
                self.store.store_result(result);
            }
            msg_types::TASK_CANCEL => {
                let task_id = envelope
                    .payload
                    .get("task_id")
                    .and_then(|v| v.as_str())
                    .ok_or("task_cancel missing task_id")?;
                tracing::info!(
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
