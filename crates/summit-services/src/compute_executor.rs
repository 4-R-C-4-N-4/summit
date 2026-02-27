//! Compute executor — polls for queued remote tasks and runs them.
//!
//! The executor picks up tasks stored by `ComputeService::handle_chunk`,
//! spawns a subprocess for each one, and sends `task_ack(Running)` then
//! `task_result` back to the submitting peer.
//!
//! Each task runs in its own subdirectory of `work_dir`. After execution,
//! any files produced in the directory are sent back to the submitter via
//! the existing file transfer infrastructure.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use tokio::sync::mpsc;
use tokio::sync::Semaphore;

use crate::chunk_types::OutgoingChunk;
use crate::compute_store::ComputeStore;
use crate::compute_types::{msg_types, ComputeEnvelope, TaskAck, TaskResult, TaskStatus};
use crate::file_transfer::chunk_file;
use crate::send_target::SendTarget;
use summit_core::config::ComputeSettings;

use std::sync::Arc;

/// Runs forever, polling the store for queued remote tasks.
pub async fn run(
    store: ComputeStore,
    settings: ComputeSettings,
    chunk_tx: mpsc::Sender<(SendTarget, OutgoingChunk)>,
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
                // Each task gets its own subdirectory for isolation.
                let task_dir = work_dir.join(&task_id[..16.min(task_id.len())]);

                // Tell the submitter we're running.
                send_ack(&chunk_tx, &peer_pubkey, &task_id, TaskStatus::Running).await;

                let start = Instant::now();
                let result_value = execute_task(&task.submit.payload, &task_dir).await;
                let elapsed_ms = start.elapsed().as_millis() as u64;

                let (status, mut result_json) = match result_value {
                    Ok(output) => (TaskStatus::Completed, output),
                    Err(err) => (TaskStatus::Failed, serde_json::json!({ "error": err })),
                };

                // Collect and send back any output files.
                let output_files = collect_output_files(&task_dir).await;
                if !output_files.is_empty() {
                    let file_names: Vec<String> = output_files
                        .iter()
                        .filter_map(|p| p.file_name().and_then(|n| n.to_str()))
                        .map(String::from)
                        .collect();

                    let mut sent = 0usize;
                    for path in &output_files {
                        match send_output_file(&chunk_tx, &peer_pubkey, path).await {
                            Ok(n) => sent += n,
                            Err(e) => {
                                tracing::warn!(
                                    path = %path.display(),
                                    error = %e,
                                    "failed to send output file"
                                );
                            }
                        }
                    }

                    // Annotate the result with the file list.
                    if let Some(obj) = result_json.as_object_mut() {
                        obj.insert("output_files".to_string(), serde_json::json!(file_names));
                        obj.insert("output_chunks_sent".to_string(), serde_json::json!(sent));
                    }

                    tracing::info!(
                        task_id = &task_id[..16.min(task_id.len())],
                        files = file_names.len(),
                        chunks = sent,
                        "output files sent to submitter"
                    );
                }

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
                send_result(&chunk_tx, &peer_pubkey, &tr).await;

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

/// Execute a task payload.
///
/// Supported formats:
///   `{"run": "hostnamectl > out.txt"}`  — shell command (via `sh -c`)
///   `{"cmd": "echo", "args": ["hi"]}`  — direct exec (no shell)
async fn execute_task(
    payload: &serde_json::Value,
    task_dir: &Path,
) -> Result<serde_json::Value, String> {
    // Ensure task directory exists.
    tokio::fs::create_dir_all(task_dir)
        .await
        .map_err(|e| format!("failed to create task dir: {e}"))?;

    let output = if let Some(run) = payload.get("run").and_then(|v| v.as_str()) {
        // Shell mode: pipes, redirections, globs all work.
        tokio::process::Command::new("sh")
            .args(["-c", run])
            .current_dir(task_dir)
            .output()
            .await
            .map_err(|e| format!("failed to spawn shell: {e}"))?
    } else if let Some(cmd) = payload.get("cmd").and_then(|v| v.as_str()) {
        // Direct exec mode: no shell interpretation.
        let args: Vec<&str> = payload
            .get("args")
            .and_then(|v| v.as_array())
            .map(|arr| arr.iter().filter_map(|v| v.as_str()).collect())
            .unwrap_or_default();

        tokio::process::Command::new(cmd)
            .args(&args)
            .current_dir(task_dir)
            .output()
            .await
            .map_err(|e| format!("failed to spawn '{}': {e}", cmd))?
    } else {
        return Err("payload must contain \"run\" (shell string) or \"cmd\" (direct exec)".into());
    };

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

/// Walk the task directory and collect all regular files.
async fn collect_output_files(task_dir: &Path) -> Vec<PathBuf> {
    let mut files = Vec::new();
    let mut stack = vec![task_dir.to_path_buf()];

    while let Some(dir) = stack.pop() {
        let mut entries = match tokio::fs::read_dir(&dir).await {
            Ok(e) => e,
            Err(_) => continue,
        };
        while let Ok(Some(entry)) = entries.next_entry().await {
            let path = entry.path();
            if let Ok(ft) = entry.file_type().await {
                if ft.is_dir() {
                    stack.push(path);
                } else if ft.is_file() {
                    files.push(path);
                }
            }
        }
    }

    files
}

/// Chunk a file and enqueue the chunks for sending to the submitter.
/// Returns the number of chunks enqueued.
async fn send_output_file(
    chunk_tx: &mpsc::Sender<(SendTarget, OutgoingChunk)>,
    peer_pubkey: &[u8; 32],
    path: &Path,
) -> Result<usize, String> {
    let chunks = chunk_file(path).map_err(|e| format!("{e}"))?;
    let count = chunks.len();
    let target = SendTarget::Peer {
        public_key: *peer_pubkey,
    };

    for chunk in chunks {
        chunk_tx
            .send((target.clone(), chunk))
            .await
            .map_err(|_| "send queue closed".to_string())?;
    }

    Ok(count)
}

// ── Helpers to send compute protocol chunks back to the submitter ────────────

async fn send_ack(
    chunk_tx: &mpsc::Sender<(SendTarget, OutgoingChunk)>,
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
    let _ = chunk_tx
        .send((
            SendTarget::Peer {
                public_key: *peer_pubkey,
            },
            chunk,
        ))
        .await;
}

async fn send_result(
    chunk_tx: &mpsc::Sender<(SendTarget, OutgoingChunk)>,
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
    let _ = chunk_tx
        .send((
            SendTarget::Peer {
                public_key: *peer_pubkey,
            },
            chunk,
        ))
        .await;
}
