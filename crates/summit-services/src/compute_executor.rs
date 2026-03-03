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
use crate::trust::{TrustLevel, TrustRegistry};
use summit_core::config::ComputeSettings;

use std::sync::Arc;

/// Runs forever, polling the store for queued remote tasks.
pub async fn run(
    store: ComputeStore,
    settings: ComputeSettings,
    chunk_tx: mpsc::Sender<(SendTarget, OutgoingChunk)>,
    trust: TrustRegistry,
) {
    let max_tasks = if settings.max_concurrent_tasks == 0 {
        std::thread::available_parallelism()
            .map(|n| n.get())
            .unwrap_or(4)
    } else {
        settings.max_concurrent_tasks as usize
    };

    let task_timeout = Duration::from_secs(if settings.task_timeout_secs == 0 {
        300
    } else {
        settings.task_timeout_secs
    });

    let max_memory_bytes = settings.max_memory_bytes;
    let max_cpu_cores = settings.max_cpu_cores;

    let semaphore = Arc::new(Semaphore::new(max_tasks));

    tracing::info!(
        max_concurrent = max_tasks,
        timeout_secs = task_timeout.as_secs(),
        max_memory_bytes,
        max_cpu_cores,
        "compute executor started"
    );

    let mut interval = tokio::time::interval(Duration::from_secs(1));

    loop {
        interval.tick().await;

        let queued = store.queued_remote_tasks();
        for task in queued {
            let task_id = task.submit.task_id.clone();
            let peer_pubkey = task.peer_pubkey;

            // Only trusted peers may execute compute tasks.
            match trust.check(&peer_pubkey) {
                TrustLevel::Trusted => {}
                level => {
                    tracing::warn!(
                        task_id = &task_id[..16.min(task_id.len())],
                        peer = hex::encode(&peer_pubkey[..8]),
                        ?level,
                        "rejecting compute task from non-trusted peer"
                    );
                    store.update_status(&task_id, TaskStatus::Failed);
                    send_ack(&chunk_tx, &peer_pubkey, &task_id, TaskStatus::Failed).await;
                    continue;
                }
            }

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
                let result_value = match tokio::time::timeout(
                    task_timeout,
                    execute_task(
                        &task.submit.payload,
                        &task_dir,
                        max_memory_bytes,
                        max_cpu_cores,
                    ),
                )
                .await
                {
                    Ok(r) => r,
                    Err(_) => Err(format!("task timed out after {}s", task_timeout.as_secs())),
                };
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

                // Clean up task directory now that output files have been sent
                if let Err(e) = tokio::fs::remove_dir_all(&task_dir).await {
                    tracing::debug!(
                        path = %task_dir.display(),
                        error = %e,
                        "failed to clean up task directory"
                    );
                }

                drop(permit);
            });
        }
    }
}

/// Apply resource limits (RLIMIT_AS for memory, RLIMIT_CPU for CPU time)
/// via `pre_exec`. Runs after fork, before exec — only affects the child.
fn apply_resource_limits(
    cmd: &mut tokio::process::Command,
    max_memory_bytes: u64,
    max_cpu_cores: u32,
) {
    let mem = max_memory_bytes;
    let cpu = max_cpu_cores;
    if mem == 0 && cpu == 0 {
        return;
    }
    // Safety: pre_exec runs between fork and exec in the child process.
    // We only call async-signal-safe libc functions (setrlimit).
    unsafe {
        cmd.pre_exec(move || {
            if mem > 0 {
                let rlim = libc::rlimit {
                    rlim_cur: mem,
                    rlim_max: mem,
                };
                if libc::setrlimit(libc::RLIMIT_AS, &rlim) != 0 {
                    return Err(std::io::Error::last_os_error());
                }
            }
            if cpu > 0 {
                // RLIMIT_CPU is in seconds. Approximate: allow cpu_cores * task_timeout
                // as total CPU seconds. Default to 600s if not otherwise bounded.
                let cpu_secs = (cpu as u64) * 600;
                let rlim = libc::rlimit {
                    rlim_cur: cpu_secs,
                    rlim_max: cpu_secs,
                };
                if libc::setrlimit(libc::RLIMIT_CPU, &rlim) != 0 {
                    return Err(std::io::Error::last_os_error());
                }
            }
            Ok(())
        });
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
    max_memory_bytes: u64,
    max_cpu_cores: u32,
) -> Result<serde_json::Value, String> {
    // Ensure task directory exists.
    tokio::fs::create_dir_all(task_dir)
        .await
        .map_err(|e| format!("failed to create task dir: {e}"))?;

    let output = if let Some(run) = payload.get("run").and_then(|v| v.as_str()) {
        // Shell mode: pipes, redirections, globs all work.
        let mut cmd = tokio::process::Command::new("sh");
        cmd.args(["-c", run]).current_dir(task_dir);
        apply_resource_limits(&mut cmd, max_memory_bytes, max_cpu_cores);
        cmd.output()
            .await
            .map_err(|e| format!("failed to spawn shell: {e}"))?
    } else if let Some(cmd_str) = payload.get("cmd").and_then(|v| v.as_str()) {
        // Direct exec mode: no shell interpretation.
        let args: Vec<&str> = payload
            .get("args")
            .and_then(|v| v.as_array())
            .map(|arr| arr.iter().filter_map(|v| v.as_str()).collect())
            .unwrap_or_default();

        let mut cmd = tokio::process::Command::new(cmd_str);
        cmd.args(&args).current_dir(task_dir);
        apply_resource_limits(&mut cmd, max_memory_bytes, max_cpu_cores);
        cmd.output()
            .await
            .map_err(|e| format!("failed to spawn '{}': {e}", cmd_str))?
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};

    static COUNTER: AtomicU64 = AtomicU64::new(0);

    fn temp_dir() -> PathBuf {
        let id = COUNTER.fetch_add(1, Ordering::Relaxed);
        std::env::temp_dir().join(format!("summit-exec-test-{}-{}", std::process::id(), id))
    }

    // ── execute_task tests ───────────────────────────────────────────────

    #[tokio::test]
    async fn execute_task_shell_echo() {
        let dir = temp_dir();
        let payload = serde_json::json!({ "run": "echo hello" });
        let result = execute_task(&payload, &dir, 0, 0).await.unwrap();
        assert_eq!(result["exit_code"], 0);
        assert!(result["stdout"].as_str().unwrap().contains("hello"));
        let _ = tokio::fs::remove_dir_all(&dir).await;
    }

    #[tokio::test]
    async fn execute_task_direct_exec() {
        let dir = temp_dir();
        let payload = serde_json::json!({ "cmd": "echo", "args": ["hi"] });
        let result = execute_task(&payload, &dir, 0, 0).await.unwrap();
        assert_eq!(result["exit_code"], 0);
        assert!(result["stdout"].as_str().unwrap().contains("hi"));
        let _ = tokio::fs::remove_dir_all(&dir).await;
    }

    #[tokio::test]
    async fn execute_task_invalid_payload() {
        let dir = temp_dir();
        let payload = serde_json::json!({ "nope": true });
        let err = execute_task(&payload, &dir, 0, 0).await.unwrap_err();
        assert!(err.contains("run"));
        assert!(err.contains("cmd"));
        let _ = tokio::fs::remove_dir_all(&dir).await;
    }

    #[tokio::test]
    async fn execute_task_failing_command() {
        let dir = temp_dir();
        let payload = serde_json::json!({ "run": "false" });
        let err = execute_task(&payload, &dir, 0, 0).await.unwrap_err();
        assert!(err.contains("exit code"));
        let _ = tokio::fs::remove_dir_all(&dir).await;
    }

    #[tokio::test]
    async fn execute_task_creates_output_file() {
        let dir = temp_dir();
        let payload = serde_json::json!({ "run": "echo data > out.txt" });
        let result = execute_task(&payload, &dir, 0, 0).await.unwrap();
        assert_eq!(result["exit_code"], 0);
        assert!(dir.join("out.txt").exists());
        let _ = tokio::fs::remove_dir_all(&dir).await;
    }

    // ── collect_output_files tests ───────────────────────────────────────

    #[tokio::test]
    async fn collect_output_files_empty_dir() {
        let dir = temp_dir();
        tokio::fs::create_dir_all(&dir).await.unwrap();
        let files = collect_output_files(&dir).await;
        assert!(files.is_empty());
        let _ = tokio::fs::remove_dir_all(&dir).await;
    }

    #[tokio::test]
    async fn collect_output_files_nested() {
        let dir = temp_dir();
        let sub = dir.join("sub");
        tokio::fs::create_dir_all(&sub).await.unwrap();
        tokio::fs::write(dir.join("a.txt"), b"a").await.unwrap();
        tokio::fs::write(sub.join("b.txt"), b"b").await.unwrap();
        let files = collect_output_files(&dir).await;
        assert_eq!(files.len(), 2);
        let _ = tokio::fs::remove_dir_all(&dir).await;
    }

    #[tokio::test]
    async fn collect_output_files_nonexistent() {
        let dir = temp_dir().join("does_not_exist");
        let files = collect_output_files(&dir).await;
        assert!(files.is_empty());
    }

    // ── trust gate rejection test ────────────────────────────────────────

    #[tokio::test]
    async fn trust_gate_rejects_untrusted_peer() {
        let store = ComputeStore::new();
        let trust = TrustRegistry::new();
        let (chunk_tx, mut chunk_rx) = mpsc::channel(16);

        let peer = [0xAAu8; 32];
        // peer is untrusted by default

        let submit = crate::compute_types::TaskSubmit {
            task_id: "untrusted-task-001".to_string(),
            sender: hex::encode(peer),
            timestamp: 100,
            payload: serde_json::json!({ "run": "echo no" }),
        };
        store.submit(peer, submit);

        // Run one poll iteration manually
        let queued = store.queued_remote_tasks();
        assert_eq!(queued.len(), 1);

        for task in queued {
            let task_id = task.submit.task_id.clone();
            let peer_pubkey = task.peer_pubkey;

            match trust.check(&peer_pubkey) {
                TrustLevel::Trusted => panic!("should not be trusted"),
                _level => {
                    store.update_status(&task_id, TaskStatus::Failed);
                    send_ack(&chunk_tx, &peer_pubkey, &task_id, TaskStatus::Failed).await;
                }
            }
        }

        // Verify task marked Failed
        let task = store.get_task("untrusted-task-001").unwrap();
        assert_eq!(task.status, TaskStatus::Failed);

        // Verify an ack chunk was sent
        let (target, _chunk) = chunk_rx.try_recv().unwrap();
        match target {
            SendTarget::Peer { public_key } => assert_eq!(public_key, peer),
            _ => panic!("expected Peer target"),
        }
    }
}
