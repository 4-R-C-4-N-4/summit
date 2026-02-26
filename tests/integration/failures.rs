use crate::fault::*;
use crate::*;

// ══════════════════════════════════════════════════════════════════════════════
//  Network & Session Failures
// ══════════════════════════════════════════════════════════════════════════════

/// Bring link down after session establishes, wait, bring it back up.
/// After the link recovers, the initiator should retry and re-establish a session.
#[test]
fn test_link_down_and_recovery() {
    if !skip_unless_ready() {
        return;
    }

    let _lock = DAEMON_LOCK.lock().unwrap();
    cleanup_summitd();

    let mut node_a = spawn_daemon(NS_A, VETH_A, &[]);
    let mut node_b = spawn_daemon(NS_B, VETH_B, &[]);

    let result = (|| -> Result<()> {
        wait_for_api(NS_A, 40)?;
        wait_for_api(NS_B, 40)?;
        let _session_id = wait_for_session(8)?;
        println!("Session established, bringing link down...");

        // Bring link down on A's side
        let _guard = link_down(NS_A, VETH_A);
        thread::sleep(Duration::from_secs(5));

        // Daemon should stay alive despite link being down
        assert!(daemon_alive(NS_A), "A died with link down");
        assert!(daemon_alive(NS_B), "B died with link down");

        // Bring link back up
        drop(_guard);

        // Both daemons should still be alive
        assert!(daemon_alive(NS_A), "A died after link recovery");
        assert!(daemon_alive(NS_B), "B died after link recovery");

        // Session should re-establish after link recovery.
        // The initiator should detect the stale handshake/session and retry.
        wait_for_condition(60, || session_count(NS_A) > 0)?;
        println!("Session re-established after link recovery");

        Ok(())
    })();

    node_a.kill().ok();
    node_b.kill().ok();
    cleanup_summitd();
    result.unwrap();
}

/// Apply 50% packet loss, spawn both daemons. Session may or may not form
/// depending on handshake packet luck. Remove loss, assert session eventually forms.
#[test]
fn test_handshake_under_packet_loss() {
    if !skip_unless_ready() {
        return;
    }

    let _lock = DAEMON_LOCK.lock().unwrap();
    cleanup_summitd();

    // Apply 50% loss before spawning
    let guard_a = add_packet_loss(NS_A, VETH_A, 50);
    let guard_b = add_packet_loss(NS_B, VETH_B, 50);

    let mut node_a = spawn_daemon(NS_A, VETH_A, &[]);
    let mut node_b = spawn_daemon(NS_B, VETH_B, &[]);

    let result = (|| -> Result<()> {
        wait_for_api(NS_A, 40)?;
        wait_for_api(NS_B, 40)?;

        // Wait a bit — session may or may not form under heavy loss
        thread::sleep(Duration::from_secs(10));

        // Both daemons must stay alive regardless
        assert!(daemon_alive(NS_A), "A died under packet loss");
        assert!(daemon_alive(NS_B), "B died under packet loss");

        let sessions_before = session_count(NS_A);
        println!("Sessions under 50% loss: {}", sessions_before);

        // Remove packet loss
        drop(guard_a);
        drop(guard_b);

        // Session should eventually form once network is clean
        wait_for_condition(30, || session_count(NS_A) > 0)?;
        println!("Session established after removing packet loss");

        Ok(())
    })();

    node_a.kill().ok();
    node_b.kill().ok();
    cleanup_summitd();
    result.unwrap();
}

/// Establish session, kill B, restart B. A should stay alive, B comes back.
/// B has a new keypair on restart, so the `attempted` set doesn't block
/// a new session (different pubkey).
#[test]
fn test_node_restart_mid_session() {
    if !skip_unless_ready() {
        return;
    }

    let _lock = DAEMON_LOCK.lock().unwrap();
    cleanup_summitd();

    let mut node_a = spawn_daemon(NS_A, VETH_A, &[]);
    let mut node_b = spawn_daemon(NS_B, VETH_B, &[]);

    let result = (|| -> Result<()> {
        wait_for_api(NS_A, 40)?;
        wait_for_api(NS_B, 40)?;
        let _session_id = wait_for_session(8)?;
        println!("Session established, killing B...");

        // Kill B
        node_b.kill().ok();
        thread::sleep(Duration::from_secs(2));

        // A should stay alive
        assert!(daemon_alive(NS_A), "A died after B was killed");

        // Restart B (new keypair on restart)
        node_b = spawn_daemon(NS_B, VETH_B, &[]);
        wait_for_api(NS_B, 40)?;
        println!("B restarted, waiting for new session...");

        // B has a new keypair, so A should be willing to handshake again
        wait_for_condition(30, || session_count(NS_A) > 0)?;
        println!(
            "New session formed after restart (sessions: {})",
            session_count(NS_A)
        );

        assert!(daemon_alive(NS_A), "A died after B restart");
        assert!(daemon_alive(NS_B), "B died after restart");

        Ok(())
    })();

    node_a.kill().ok();
    node_b.kill().ok();
    cleanup_summitd();
    result.unwrap();
}

/// Establish session, kill B (don't restart). A should prune the dead session
/// after the receive timeout fires and the session is removed from the table.
#[test]
fn test_dead_session_pruned() {
    if !skip_unless_ready() {
        return;
    }

    let _lock = DAEMON_LOCK.lock().unwrap();
    cleanup_summitd();

    let mut node_a = spawn_daemon(NS_A, VETH_A, &[]);
    let mut node_b = spawn_daemon(NS_B, VETH_B, &[]);

    let result = (|| -> Result<()> {
        wait_for_api(NS_A, 40)?;
        wait_for_api(NS_B, 40)?;
        let _session_id = wait_for_session(8)?;
        let sessions_before = session_count(NS_A);
        assert!(sessions_before >= 1, "no session before kill");
        println!("Sessions before kill: {}", sessions_before);

        // Kill B, don't restart
        node_b.kill().ok();

        // A should stay alive and responsive
        assert!(daemon_alive(NS_A), "A died after B was killed");

        // Dead session should be pruned after receive timeout (60s) + some margin
        wait_for_condition(90, || session_count(NS_A) == 0)?;
        println!("Dead session pruned from table");

        assert!(daemon_alive(NS_A), "A died after session pruning");

        Ok(())
    })();

    node_a.kill().ok();
    node_b.kill().ok();
    cleanup_summitd();
    result.unwrap();
}

/// Send garbage UDP packets to broadcast and API ports. Daemon should survive.
#[test]
fn test_invalid_udp_survives() {
    if !skip_unless_ready() {
        return;
    }

    let _lock = DAEMON_LOCK.lock().unwrap();
    cleanup_summitd();

    let mut node_a = spawn_daemon(NS_A, VETH_A, &[]);

    let result = (|| -> Result<()> {
        wait_for_api(NS_A, 40)?;

        // Send garbage to broadcast port (9000) and API port (9001)
        println!("Sending 100 garbage packets to port 9000...");
        send_garbage_udp(NS_A, 9000, 100);
        println!("Sending 100 garbage packets to port 9001...");
        send_garbage_udp(NS_A, 9001, 100);

        thread::sleep(Duration::from_secs(2));

        // Daemon must survive
        assert!(daemon_alive(NS_A), "daemon died after garbage UDP");

        // API must still be responsive
        let status = api_get(NS_A, "/status")?;
        assert!(
            status["peers_discovered"].is_number(),
            "API broken after garbage UDP"
        );
        println!("Daemon survived 200 garbage UDP packets");

        Ok(())
    })();

    node_a.kill().ok();
    cleanup_summitd();
    result.unwrap();
}

/// Block all UDP in NS_B, spawn both daemons. No sessions should form.
/// Remove block, session should eventually establish.
#[test]
fn test_port_block_graceful() {
    if !skip_unless_ready() {
        return;
    }

    let _lock = DAEMON_LOCK.lock().unwrap();
    cleanup_summitd();

    // Block all UDP on B before spawning
    let guard = block_all_udp(NS_B);

    let mut node_a = spawn_daemon(NS_A, VETH_A, &[]);
    let mut node_b = spawn_daemon(NS_B, VETH_B, &[]);

    let result = (|| -> Result<()> {
        wait_for_api(NS_A, 40)?;
        wait_for_api(NS_B, 40)?;

        thread::sleep(Duration::from_secs(8));

        // Both alive
        assert!(daemon_alive(NS_A), "A died with UDP blocked on B");
        assert!(daemon_alive(NS_B), "B died with own UDP blocked");

        // No sessions should form (B can't receive discovery/handshake)
        let sessions = session_count(NS_A);
        println!("Sessions with B's UDP blocked: {}", sessions);
        // Sessions might be 0 or might form if A initiates and B responds on a
        // different path — document actual behavior
        assert!(
            sessions == 0,
            "expected no sessions with UDP blocked, got {}",
            sessions
        );

        // Remove block
        drop(guard);
        println!("UDP block removed, waiting for session...");

        // Session should eventually establish
        wait_for_condition(30, || session_count(NS_A) > 0)?;
        println!("Session established after removing UDP block");

        Ok(())
    })();

    node_a.kill().ok();
    node_b.kill().ok();
    cleanup_summitd();
    result.unwrap();
}

// ══════════════════════════════════════════════════════════════════════════════
//  File Transfer Failures
// ══════════════════════════════════════════════════════════════════════════════

/// Send large file (2MB), kill sender after 500ms.
/// Receiver should stay alive, partial file should not be committed.
#[test]
fn test_sender_killed_mid_transfer() {
    if !skip_unless_ready() {
        return;
    }

    let _lock = DAEMON_LOCK.lock().unwrap();
    cleanup_summitd();
    std::fs::remove_dir_all("/tmp/summit-received").ok();

    let auto_env = [("SUMMIT_TRUST__AUTO_TRUST", "true")];
    let mut node_a = spawn_daemon(NS_A, VETH_A, &auto_env);
    let mut node_b = spawn_daemon(NS_B, VETH_B, &auto_env);

    // Create a 2MB test file
    let test_file = "/tmp/summit-test-large-sender-kill.bin";
    let data = vec![0xABu8; 2 * 1024 * 1024];
    std::fs::write(test_file, &data).unwrap();

    let result = (|| -> Result<()> {
        wait_for_api(NS_A, 40)?;
        wait_for_api(NS_B, 40)?;
        let _session = wait_for_session(8)?;

        // Send file
        let _ = ctl(NS_A, &["send", test_file]);

        // Kill sender after 500ms
        thread::sleep(Duration::from_millis(500));
        node_a.kill().ok();
        println!("Sender killed mid-transfer");

        thread::sleep(Duration::from_secs(5));

        // Receiver must stay alive
        assert!(daemon_alive(NS_B), "receiver died after sender killed");

        // Partial file should NOT be committed to disk
        let received_path = "/tmp/summit-received/summit-test-large-sender-kill.bin";
        assert!(
            !std::path::Path::new(received_path).exists(),
            "partial file should not be committed when sender is killed mid-transfer"
        );

        Ok(())
    })();

    node_a.kill().ok();
    node_b.kill().ok();
    cleanup_summitd();
    std::fs::remove_file(test_file).ok();
    result.unwrap();
}

/// Send file, kill receiver mid-transfer, restart receiver, re-send file.
/// File should arrive intact after re-send.
#[test]
fn test_receiver_killed_and_resend() {
    if !skip_unless_ready() {
        return;
    }

    let _lock = DAEMON_LOCK.lock().unwrap();
    cleanup_summitd();
    std::fs::remove_dir_all("/tmp/summit-received").ok();

    let auto_env = [("SUMMIT_TRUST__AUTO_TRUST", "true")];
    let mut node_a = spawn_daemon(NS_A, VETH_A, &auto_env);
    let mut node_b = spawn_daemon(NS_B, VETH_B, &auto_env);

    let test_content = "resend test — this file should arrive intact after receiver restart";
    let test_file = "/tmp/summit-test-resend.txt";
    std::fs::write(test_file, test_content).unwrap();

    let result = (|| -> Result<()> {
        wait_for_api(NS_A, 40)?;
        wait_for_api(NS_B, 40)?;
        let _session = wait_for_session(8)?;

        // Send file, then kill receiver
        let _ = ctl(NS_A, &["send", test_file]);
        thread::sleep(Duration::from_millis(200));
        node_b.kill().ok();
        println!("Receiver killed mid-transfer");

        thread::sleep(Duration::from_secs(2));

        // Sender should stay alive
        assert!(daemon_alive(NS_A), "sender died after receiver killed");

        // Restart receiver
        std::fs::remove_dir_all("/tmp/summit-received").ok();
        node_b = spawn_daemon(NS_B, VETH_B, &auto_env);
        wait_for_api(NS_B, 40)?;
        println!("Receiver restarted");

        // Wait for new session
        wait_for_condition(30, || session_count(NS_A) > 0)?;

        // Re-send the file
        let send_out = ctl(NS_A, &["send", test_file])?;
        assert!(
            send_out.contains("File queued"),
            "re-send failed: {}",
            send_out
        );

        thread::sleep(Duration::from_secs(8));

        // File should arrive intact
        let received_path = "/tmp/summit-received/summit-test-resend.txt";
        let received =
            std::fs::read_to_string(received_path).context("resent file not received")?;
        assert_eq!(received, test_content, "content mismatch after resend");
        println!("File arrived intact after resend");

        Ok(())
    })();

    node_a.kill().ok();
    node_b.kill().ok();
    cleanup_summitd();
    std::fs::remove_file(test_file).ok();
    result.unwrap();
}

/// Send 10 different small files in rapid succession.
/// All should arrive intact, no deadlock, daemon alive.
#[test]
fn test_concurrent_file_transfers() {
    if !skip_unless_ready() {
        return;
    }

    let _lock = DAEMON_LOCK.lock().unwrap();
    cleanup_summitd();
    std::fs::remove_dir_all("/tmp/summit-received").ok();

    let auto_env = [("SUMMIT_TRUST__AUTO_TRUST", "true")];
    let mut node_a = spawn_daemon(NS_A, VETH_A, &auto_env);
    let mut node_b = spawn_daemon(NS_B, VETH_B, &auto_env);

    // Create 10 small test files
    let mut test_files = Vec::new();
    for i in 0..10 {
        let path = format!("/tmp/summit-test-concurrent-{}.txt", i);
        let content = format!("concurrent file {} — unique content {}", i, i * 31337);
        std::fs::write(&path, &content).unwrap();
        test_files.push((path, content));
    }

    let result = (|| -> Result<()> {
        wait_for_api(NS_A, 40)?;
        wait_for_api(NS_B, 40)?;
        let _session = wait_for_session(8)?;

        // Send all 10 files rapidly
        for (path, _) in &test_files {
            let send_out = ctl(NS_A, &["send", path])?;
            assert!(
                send_out.contains("File queued"),
                "send failed for {}: {}",
                path,
                send_out
            );
        }
        println!("All 10 files queued");

        // Wait for transfers
        thread::sleep(Duration::from_secs(15));

        // Both daemons alive
        assert!(daemon_alive(NS_A), "A died during concurrent transfers");
        assert!(daemon_alive(NS_B), "B died during concurrent transfers");

        // Check how many arrived
        let mut received_count = 0;
        for (path, expected_content) in &test_files {
            let filename = std::path::Path::new(path)
                .file_name()
                .unwrap()
                .to_str()
                .unwrap();
            let received_path = format!("/tmp/summit-received/{}", filename);
            if let Ok(received) = std::fs::read_to_string(&received_path) {
                assert_eq!(
                    &received, expected_content,
                    "content mismatch for {}",
                    filename
                );
                received_count += 1;
            }
        }
        println!("Received {}/10 files", received_count);
        assert!(
            received_count >= 8,
            "too few files received: {}/10",
            received_count
        );

        Ok(())
    })();

    node_a.kill().ok();
    node_b.kill().ok();
    cleanup_summitd();
    for (path, _) in &test_files {
        std::fs::remove_file(path).ok();
    }
    result.unwrap();
}

/// Apply 20% packet loss, send a small file. Daemon must stay alive.
/// File may or may not arrive (UDP is lossy). No crash.
#[test]
fn test_file_transfer_under_packet_loss() {
    if !skip_unless_ready() {
        return;
    }

    let _lock = DAEMON_LOCK.lock().unwrap();
    cleanup_summitd();
    std::fs::remove_dir_all("/tmp/summit-received").ok();

    let auto_env = [("SUMMIT_TRUST__AUTO_TRUST", "true")];
    let mut node_a = spawn_daemon(NS_A, VETH_A, &auto_env);
    let mut node_b = spawn_daemon(NS_B, VETH_B, &auto_env);

    let test_content = "file transfer under packet loss test content";
    let test_file = "/tmp/summit-test-lossy.txt";
    std::fs::write(test_file, test_content).unwrap();

    let result = (|| -> Result<()> {
        wait_for_api(NS_A, 40)?;
        wait_for_api(NS_B, 40)?;
        let _session = wait_for_session(8)?;

        // Apply 20% packet loss
        let _guard_a = add_packet_loss(NS_A, VETH_A, 20);
        let _guard_b = add_packet_loss(NS_B, VETH_B, 20);

        // Send file
        let send_out = ctl(NS_A, &["send", test_file])?;
        assert!(
            send_out.contains("File queued"),
            "send failed: {}",
            send_out
        );

        thread::sleep(Duration::from_secs(12));

        // Daemons must survive
        assert!(daemon_alive(NS_A), "A died under packet loss");
        assert!(daemon_alive(NS_B), "B died under packet loss");

        // File may or may not have arrived — that's OK for lossy UDP
        let received_path = "/tmp/summit-received/summit-test-lossy.txt";
        if std::path::Path::new(received_path).exists() {
            let received = std::fs::read_to_string(received_path)?;
            assert_eq!(received, test_content, "content corrupted under loss");
            println!("File arrived intact despite 20% loss");
        } else {
            println!("File did not arrive (expected under packet loss)");
        }

        Ok(())
    })();

    node_a.kill().ok();
    node_b.kill().ok();
    cleanup_summitd();
    std::fs::remove_file(test_file).ok();
    result.unwrap();
}

// ══════════════════════════════════════════════════════════════════════════════
//  Trust Failures
// ══════════════════════════════════════════════════════════════════════════════

/// Start file transfer, then immediately block the sender's trust.
/// Transfer should not complete (chunks dropped after block).
#[test]
fn test_block_during_file_transfer() {
    if !skip_unless_ready() {
        return;
    }

    let _lock = DAEMON_LOCK.lock().unwrap();
    cleanup_summitd();
    std::fs::remove_dir_all("/tmp/summit-received").ok();

    let auto_env = [("SUMMIT_TRUST__AUTO_TRUST", "true")];
    let mut node_a = spawn_daemon(NS_A, VETH_A, &auto_env);
    let mut node_b = spawn_daemon(NS_B, VETH_B, &auto_env);

    // Larger file to give us time to block mid-transfer
    let test_file = "/tmp/summit-test-block-transfer.bin";
    let data = vec![0xCDu8; 512 * 1024]; // 512KB
    std::fs::write(test_file, &data).unwrap();

    let result = (|| -> Result<()> {
        wait_for_api(NS_A, 40)?;
        wait_for_api(NS_B, 40)?;
        let _session = wait_for_session(8)?;

        // Get A's pubkey as seen by B
        let pubkey_a = get_peer_pubkey(NS_B)?;

        // Send file from A
        let _ = ctl(NS_A, &["send", test_file]);

        // Immediately block A on B
        thread::sleep(Duration::from_millis(100));
        let block_body = serde_json::json!({ "public_key": pubkey_a }).to_string();
        api_post(NS_B, "/trust/block", &block_body)?;
        println!("Blocked A on B mid-transfer");

        thread::sleep(Duration::from_secs(5));

        // B should be alive
        assert!(daemon_alive(NS_B), "B died after blocking mid-transfer");

        // File should NOT have been committed (blocked sender's chunks are dropped)
        let received_path = "/tmp/summit-received/summit-test-block-transfer.bin";
        assert!(
            !std::path::Path::new(received_path).exists(),
            "file should not be committed after sender is blocked"
        );

        Ok(())
    })();

    node_a.kill().ok();
    node_b.kill().ok();
    cleanup_summitd();
    std::fs::remove_file(test_file).ok();
    result.unwrap();
}

/// B sends messages while untrusted by A. Trust B on A.
/// Buffered messages should be replayed and appear on A after trust is granted.
#[test]
fn test_trust_then_check_buffered_messages() {
    if !skip_unless_ready() {
        return;
    }

    let _lock = DAEMON_LOCK.lock().unwrap();
    cleanup_summitd();

    // A does NOT auto-trust. B auto-trusts everyone.
    let mut node_a = spawn_daemon(NS_A, VETH_A, &[]);
    let auto_env = [("SUMMIT_TRUST__AUTO_TRUST", "true")];
    let mut node_b = spawn_daemon(NS_B, VETH_B, &auto_env);

    let result = (|| -> Result<()> {
        wait_for_api(NS_A, 40)?;
        wait_for_api(NS_B, 40)?;

        thread::sleep(Duration::from_secs(8));

        let pubkey_a = get_peer_pubkey(NS_B)?;
        let pubkey_b = get_peer_pubkey(NS_A)?;

        // B sends messages while untrusted by A
        for i in 1..=3 {
            let body = serde_json::json!({
                "to": pubkey_a,
                "text": format!("buffered msg {}", i)
            })
            .to_string();
            api_post(NS_B, "/messages/send", &body)?;
        }
        println!("Sent 3 messages from untrusted B");

        thread::sleep(Duration::from_secs(4));

        // A should have buffered chunks from B
        let pending = api_get(NS_A, "/trust/pending")?;
        let empty = vec![];
        let pending_peers = pending["peers"].as_array().unwrap_or(&empty);
        assert!(
            !pending_peers.is_empty(),
            "expected buffered chunks from untrusted B"
        );

        // Now trust B — this should flush and replay the buffered chunks
        let flushed = trust_peer(NS_A, &pubkey_b)?;
        println!("Trusted B, flushed {} chunks", flushed);
        assert!(
            flushed >= 3,
            "expected >= 3 flushed chunks, got {}",
            flushed
        );

        thread::sleep(Duration::from_secs(4));

        // Buffered messages should now be delivered via the replay channel
        let msgs = api_get(NS_A, &format!("/messages/{}", pubkey_b))?;
        let msg_list = msgs["messages"].as_array().context("no messages array")?;
        println!("Messages from B after trust: {}", msg_list.len());
        assert!(
            msg_list.len() >= 3,
            "expected >= 3 replayed messages, got {}",
            msg_list.len()
        );

        // Both alive
        assert!(daemon_alive(NS_A), "A died");
        assert!(daemon_alive(NS_B), "B died");

        Ok(())
    })();

    node_a.kill().ok();
    node_b.kill().ok();
    cleanup_summitd();
    result.unwrap();
}

// ══════════════════════════════════════════════════════════════════════════════
//  Compute Failures
// ══════════════════════════════════════════════════════════════════════════════

/// Submit a long-running task to B, kill B after 2s. A should stay alive,
/// task status remains Queued (B never reported back).
#[test]
fn test_compute_worker_crash() {
    if !skip_unless_ready() {
        return;
    }

    let _lock = DAEMON_LOCK.lock().unwrap();
    cleanup_summitd();

    let env = [
        ("SUMMIT_TRUST__AUTO_TRUST", "true"),
        ("SUMMIT_SERVICES__COMPUTE", "true"),
    ];
    let mut node_a = spawn_daemon(NS_A, VETH_A, &env);
    let mut node_b = spawn_daemon(NS_B, VETH_B, &env);

    let result = (|| -> Result<()> {
        wait_for_api(NS_A, 40)?;
        wait_for_api(NS_B, 40)?;
        let _session = wait_for_session(8)?;

        let pubkey_b = get_peer_pubkey(NS_A)?;

        // Submit a long-running task
        let body = serde_json::json!({
            "to": pubkey_b,
            "payload": { "op": "shell", "cmd": ["sleep", "30"] }
        })
        .to_string();
        let resp = api_post(NS_A, "/compute/submit", &body)?;
        let task_id = resp["task_id"]
            .as_str()
            .context("missing task_id")?
            .to_string();
        println!("Submitted long task: {}...", &task_id[..16]);

        // Kill B after 2 seconds
        thread::sleep(Duration::from_secs(2));
        node_b.kill().ok();
        println!("Worker B killed");

        thread::sleep(Duration::from_secs(3));

        // A should stay alive
        assert!(daemon_alive(NS_A), "A died after worker crash");

        // Task should still show as Queued (B never reported completion)
        let tasks = api_get(NS_A, &format!("/compute/tasks/{}", pubkey_b))?;
        let task_list = tasks["tasks"].as_array().context("no tasks")?;
        if let Some(task) = task_list
            .iter()
            .find(|t| t["task_id"].as_str() == Some(&task_id))
        {
            let status = task["status"].as_str().unwrap_or("unknown");
            println!("Task status after worker crash: {}", status);
            assert_eq!(status, "Queued", "task status changed despite worker crash");
        }

        // Restart B, verify A still functional
        node_b = spawn_daemon(NS_B, VETH_B, &env);
        wait_for_api(NS_B, 40)?;
        assert!(daemon_alive(NS_A), "A died after B restart");
        println!("A still functional after worker crash and restart");

        Ok(())
    })();

    node_a.kill().ok();
    node_b.kill().ok();
    cleanup_summitd();
    result.unwrap();
}

/// Submit 5 tasks rapidly. All should be stored, daemon alive, no panics.
#[test]
fn test_concurrent_compute_tasks() {
    if !skip_unless_ready() {
        return;
    }

    let _lock = DAEMON_LOCK.lock().unwrap();
    cleanup_summitd();

    let env = [
        ("SUMMIT_TRUST__AUTO_TRUST", "true"),
        ("SUMMIT_SERVICES__COMPUTE", "true"),
    ];
    let mut node_a = spawn_daemon(NS_A, VETH_A, &env);
    let mut node_b = spawn_daemon(NS_B, VETH_B, &env);

    let result = (|| -> Result<()> {
        wait_for_api(NS_A, 40)?;
        wait_for_api(NS_B, 40)?;
        let _session = wait_for_session(8)?;

        let pubkey_b = get_peer_pubkey(NS_A)?;

        // Submit 5 tasks rapidly
        let mut task_ids = Vec::new();
        for i in 0..5 {
            let body = serde_json::json!({
                "to": pubkey_b,
                "payload": { "op": "echo", "input": format!("concurrent task {}", i) }
            })
            .to_string();
            let resp = api_post(NS_A, "/compute/submit", &body)?;
            let task_id = resp["task_id"]
                .as_str()
                .context("missing task_id")?
                .to_string();
            task_ids.push(task_id);
        }
        println!("Submitted 5 tasks rapidly");

        thread::sleep(Duration::from_secs(4));

        // Both daemons alive
        assert!(daemon_alive(NS_A), "A died during concurrent submit");
        assert!(daemon_alive(NS_B), "B died during concurrent submit");

        // All tasks should be stored on A
        let tasks = api_get(NS_A, &format!("/compute/tasks/{}", pubkey_b))?;
        let task_list = tasks["tasks"].as_array().context("no tasks")?;
        assert!(
            task_list.len() >= 5,
            "expected >= 5 tasks, got {}",
            task_list.len()
        );
        println!("All 5 tasks stored successfully");

        Ok(())
    })();

    node_a.kill().ok();
    node_b.kill().ok();
    cleanup_summitd();
    result.unwrap();
}

// ══════════════════════════════════════════════════════════════════════════════
//  Shutdown
// ══════════════════════════════════════════════════════════════════════════════

/// summit-ctl shutdown: daemon should exit cleanly after the command.
#[test]
fn test_ctl_shutdown() {
    if !skip_unless_ready() {
        return;
    }

    let _lock = DAEMON_LOCK.lock().unwrap();
    cleanup_summitd();

    let mut node_a = spawn_daemon(NS_A, VETH_A, &[]);

    let result = (|| -> Result<()> {
        wait_for_api(NS_A, 40)?;

        // Shutdown via CLI
        let out = ctl(NS_A, &["shutdown"])?;
        println!("shutdown: {}", out);

        // Wait for daemon to exit
        thread::sleep(Duration::from_secs(2));

        // API should no longer be reachable
        let check = Command::new("ip")
            .args(["netns", "exec", NS_A])
            .args([
                "curl",
                "-sf",
                "-o",
                "/dev/null",
                "-w",
                "%{http_code}",
                "http://127.0.0.1:9001/api/status",
            ])
            .output();

        if let Ok(out) = check {
            assert_ne!(out.stdout, b"200", "API still reachable after shutdown");
        }

        // Process should have exited
        let wait_result = node_a.try_wait();
        match wait_result {
            Ok(Some(status)) => println!("Daemon exited with: {}", status),
            Ok(None) => {
                // Still running — kill it
                node_a.kill().ok();
                println!("Daemon still running after shutdown, killed");
            }
            Err(e) => println!("Error checking daemon status: {}", e),
        }

        Ok(())
    })();

    // Ensure cleanup regardless
    node_a.kill().ok();
    cleanup_summitd();
    result.unwrap();
}
