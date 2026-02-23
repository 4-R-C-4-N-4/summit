//! Compute service wire types — task submission, acknowledgment, and results.
//!
//! `ComputeEnvelope` is the JSON wire format for all compute chunks.
//! Payloads are kept as `serde_json::Value` — actual compute semantics are future work.

use serde::{Deserialize, Serialize};

// ── Envelope ──────────────────────────────────────────────────────────────────

/// JSON envelope — the payload of every compute chunk.
///
/// Discriminated by `msg_type`. Receivers dispatch on this field and
/// deserialize `payload` according to the type.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ComputeEnvelope {
    /// Discriminator: "task_submit", "task_ack", "task_result", "task_cancel".
    pub msg_type: String,
    /// Type-specific content. Structure is defined by `msg_type`.
    pub payload: serde_json::Value,
}

/// Well-known `msg_type` strings.
pub mod msg_types {
    pub const TASK_SUBMIT: &str = "task_submit";
    pub const TASK_ACK: &str = "task_ack";
    pub const TASK_RESULT: &str = "task_result";
    pub const TASK_CANCEL: &str = "task_cancel";
}

// ── Message payloads ──────────────────────────────────────────────────────────

/// Task submission — sent by the client to request execution.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskSubmit {
    /// Hex-encoded BLAKE3 hash identifying this task (deduplication key).
    pub task_id: String,
    /// Sender public key, hex-encoded.
    pub sender: String,
    /// Unix timestamp in milliseconds.
    pub timestamp: u64,
    /// Opaque task definition. Structure is defined by the execution engine (future work).
    pub payload: serde_json::Value,
}

/// Peer acknowledgment of task receipt.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskAck {
    /// Task being acknowledged.
    pub task_id: String,
    /// Current status of the task.
    pub status: TaskStatus,
}

/// Task result — returned when execution completes or fails.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskResult {
    /// Completed task.
    pub task_id: String,
    /// Opaque result value. Structure is defined by the execution engine (future work).
    pub result: serde_json::Value,
    /// Wall-clock milliseconds elapsed during execution.
    pub elapsed_ms: u64,
}

/// Lifecycle status of a compute task.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TaskStatus {
    Queued,
    Running,
    Completed,
    Failed,
    Cancelled,
}
