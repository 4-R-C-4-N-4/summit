//! Compute executor — polls for queued remote tasks and runs them.
//!
//! The executor picks up tasks stored by `ComputeService::handle_chunk`,
//! spawns a subprocess for each one, and sends `task_ack(Running)` then
//! `task_result` back to the submitting peer.

use std::time::{Duration, Instant};

use tokio::sync::mpsc;
use tokio::sync::Semaphore;

use crate::chunk_types::OutgoingChunk;
use crate::compute_store::ComputeStore;
use crate::compute_types::{msg_types, ComputeEnvelope, TaskAck, TaskResult, TaskStatus};
use crate::send_target::SendTarget;
use summit_core::config::ComputeSettings;

use std::sync::Arc;

/// Runs forever, polling the store for queued remote tasks.
pub async fn run(
    store: ComputeStore,
    settings: ComputeSettings,
    chunk_tx: mpsc::UnboundedSender<(SendTarget, OutgoingChunk)>,
) {
    let max_tasks = if settings.max_concurrent_tasks == 0 {
        std::thread::available_parallelism()
            .map(|n| n.get())
            .unwrap_or(4)
    } else {
        settings.max_concurrent_tasks as usize
    };

    let semaphore = Arc::new(Semaphore::new(max_tasks));

    tracing::info!(max_concurrent = max_tasks, "compute executor started");

    let mut interval = tokio::time::interval(Duration::from_secs(1));

    loop {
        interval.tick().await;

        let queued = store.queued_remote_tasks();
        for task in queued {
            let task_id = task.submit.task_id.clone();
            let peer_pubkey = task.peer_pubkey;

            // Mark running immediately so the next poll doesn't re-pick it.
            store.update_status(&task_id, TaskStatus::Running);

            let permit = match semaphore.clone().acquire_owned().await {
                Ok(p) => p,
                Err(_) => break, // semaphore closed
            };

            let store = store.clone();
            let chunk_tx = chunk_tx.clone();
            let work_dir = settings.work_dir.clone();

            tokio::spawn(async move {
                // Tell the submitter we're running.
                send_ack(&chunk_tx, &peer_pubkey, &task_id, TaskStatus::Running);

                let start = Instant::now();
                let result_value = execute_task(&task.submit.payload, &work_dir).await;
                let elapsed_ms = start.elapsed().as_millis() as u64;

                let (status, result_json) = match result_value {
                    Ok(output) => (TaskStatus::Completed, output),
                    Err(err) => (TaskStatus::Failed, serde_json::json!({ "error": err })),
                };

                // Update local store.
                if status == TaskStatus::Completed {
                    store.store_result(TaskResult {
                        task_id: task_id.clone(),
                        result: result_json.clone(),
                        elapsed_ms,
                    });
                } else {
                    store.update_status(&task_id, status);
                }

                // Send result back to submitter.
                let tr = TaskResult {
                    task_id: task_id.clone(),
                    result: result_json,
                    elapsed_ms,
                };
                send_result(&chunk_tx, &peer_pubkey, &tr);

                tracing::info!(
                    task_id = &task_id[..16.min(task_id.len())],
                    ?status,
                    elapsed_ms,
                    "compute task finished"
                );

                drop(permit);
            });
        }
    }
}

/// Execute a task payload. Expected shape: `{"cmd": "...", "args": ["..."]}`.
async fn execute_task(
    payload: &serde_json::Value,
    work_dir: &std::path::Path,
) -> Result<serde_json::Value, String> {
    let cmd = payload
        .get("cmd")
        .and_then(|v| v.as_str())
        .ok_or_else(|| "payload missing \"cmd\" string".to_string())?;

    let args: Vec<&str> = payload
        .get("args")
        .and_then(|v| v.as_array())
        .map(|arr| arr.iter().filter_map(|v| v.as_str()).collect())
        .unwrap_or_default();

    // Ensure work_dir exists.
    let _ = tokio::fs::create_dir_all(work_dir).await;

    let output = tokio::process::Command::new(cmd)
        .args(&args)
        .current_dir(work_dir)
        .output()
        .await
        .map_err(|e| format!("failed to spawn '{}': {}", cmd, e))?;

    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();

    if output.status.success() {
        Ok(serde_json::json!({
            "exit_code": 0,
            "stdout": stdout,
            "stderr": stderr,
        }))
    } else {
        let code = output.status.code().unwrap_or(-1);
        Err(format!(
            "exit code {}: {}",
            code,
            if stderr.is_empty() { &stdout } else { &stderr }
        ))
    }
}

// ── Helpers to send chunks back to the submitter ─────────────────────────────

fn send_ack(
    chunk_tx: &mpsc::UnboundedSender<(SendTarget, OutgoingChunk)>,
    peer_pubkey: &[u8; 32],
    task_id: &str,
    status: TaskStatus,
) {
    let ack = TaskAck {
        task_id: task_id.to_string(),
        status,
    };
    let envelope = ComputeEnvelope {
        msg_type: msg_types::TASK_ACK.to_string(),
        payload: match serde_json::to_value(&ack) {
            Ok(v) => v,
            Err(_) => return,
        },
    };
    let raw = match serde_json::to_vec(&envelope) {
        Ok(v) => v,
        Err(_) => return,
    };
    let chunk = OutgoingChunk {
        type_tag: 0,
        schema_id: summit_core::wire::compute_hash(),
        payload: bytes::Bytes::from(raw),
        priority_flags: 0x02,
    };
    let _ = chunk_tx.send((
        SendTarget::Peer {
            public_key: *peer_pubkey,
        },
        chunk,
    ));
}

fn send_result(
    chunk_tx: &mpsc::UnboundedSender<(SendTarget, OutgoingChunk)>,
    peer_pubkey: &[u8; 32],
    result: &TaskResult,
) {
    let envelope = ComputeEnvelope {
        msg_type: msg_types::TASK_RESULT.to_string(),
        payload: match serde_json::to_value(result) {
            Ok(v) => v,
            Err(_) => return,
        },
    };
    let raw = match serde_json::to_vec(&envelope) {
        Ok(v) => v,
        Err(_) => return,
    };
    let chunk = OutgoingChunk {
        type_tag: 0,
        schema_id: summit_core::wire::compute_hash(),
        payload: bytes::Bytes::from(raw),
        priority_flags: 0x02,
    };
    let _ = chunk_tx.send((
        SendTarget::Peer {
            public_key: *peer_pubkey,
        },
        chunk,
    ));
}
