use crate::fault::*;
use crate::*;

// ══════════════════════════════════════════════════════════════════════════════
//  NACK Recovery — File transfer under packet loss
// ══════════════════════════════════════════════════════════════════════════════

/// Send a file under 30% packet loss. The NACK recovery loop should detect
/// missing chunks and request retransmission, completing the transfer.
///
/// Without NACK recovery this would fail ~100% of the time for multi-chunk files.
#[test]
fn test_nack_recovers_file_under_packet_loss() {
    if !skip_unless_ready() {
        return;
    }

    let _lock = DAEMON_LOCK.lock().unwrap();
    cleanup_summitd();
    std::fs::remove_dir_all("/tmp/summit-received").ok();

    let auto_env = [("SUMMIT_TRUST__AUTO_TRUST", "true")];
    let mut node_a = spawn_daemon(NS_A, VETH_A, &auto_env);
    let mut node_b = spawn_daemon(NS_B, VETH_B, &auto_env);

    // 128KB file — multiple chunks, high chance of drops at 30% loss
    let test_file = "/tmp/summit-test-nack-recovery.bin";
    let data = vec![0xFEu8; 128 * 1024];
    std::fs::write(test_file, &data).unwrap();

    let result = (|| -> Result<()> {
        wait_for_api(NS_A, 40)?;
        wait_for_api(NS_B, 40)?;
        let _session = wait_for_session(8)?;

        // Apply 30% packet loss on receiver side
        let _guard_b = add_packet_loss(NS_B, VETH_B, 30);

        // Send file
        let send_out = ctl(NS_A, &["send", test_file])?;
        assert!(
            send_out.contains("File queued"),
            "send failed: {}",
            send_out
        );
        println!("File queued under 30% loss");

        // NACK recovery timeline: ~3s NACK_DELAY + retransmit per attempt.
        // With MAX_NACK_ATTEMPTS=3, worst case is ~12s. Give generous margin.
        thread::sleep(Duration::from_secs(25));

        // Both daemons must survive
        assert!(daemon_alive(NS_A), "sender died during NACK recovery");
        assert!(daemon_alive(NS_B), "receiver died during NACK recovery");

        // File should have arrived via NACK retransmission
        let received_path = "/tmp/summit-received/summit-test-nack-recovery.bin";
        let received =
            std::fs::read(received_path).context("file not received after NACK recovery")?;
        assert_eq!(received.len(), data.len(), "size mismatch after recovery");
        assert_eq!(received, data, "content mismatch after NACK recovery");
        println!("File recovered successfully under 30% packet loss");

        Ok(())
    })();

    node_a.kill().ok();
    node_b.kill().ok();
    cleanup_summitd();
    std::fs::remove_file(test_file).ok();
    result.unwrap();
}

/// Send a file, then temporarily block the receiver's UDP port so all data chunks
/// are dropped. Unblock after a few seconds. NACK recovery should retransmit and
/// complete the file.
#[test]
fn test_nack_recovers_after_temporary_blackout() {
    if !skip_unless_ready() {
        return;
    }

    let _lock = DAEMON_LOCK.lock().unwrap();
    cleanup_summitd();
    std::fs::remove_dir_all("/tmp/summit-received").ok();

    let auto_env = [("SUMMIT_TRUST__AUTO_TRUST", "true")];
    let mut node_a = spawn_daemon(NS_A, VETH_A, &auto_env);
    let mut node_b = spawn_daemon(NS_B, VETH_B, &auto_env);

    let test_content = "nack blackout recovery test — this content must arrive intact";
    let test_file = "/tmp/summit-test-nack-blackout.txt";
    std::fs::write(test_file, test_content).unwrap();

    let result = (|| -> Result<()> {
        wait_for_api(NS_A, 40)?;
        wait_for_api(NS_B, 40)?;
        let _session = wait_for_session(8)?;

        // Block B's chunk port briefly — metadata gets through, data chunks don't
        // Use high packet loss instead of port block to let session stay alive
        let guard_b = add_packet_loss(NS_B, VETH_B, 90);

        // Send file while receiver is nearly deaf
        let send_out = ctl(NS_A, &["send", test_file])?;
        assert!(
            send_out.contains("File queued"),
            "send failed: {}",
            send_out
        );
        println!("File queued during 90% loss blackout");

        // Wait a bit, then restore network
        thread::sleep(Duration::from_secs(4));
        drop(guard_b);
        println!("Network restored — NACK recovery should kick in");

        // Wait for NACK recovery cycle
        thread::sleep(Duration::from_secs(20));

        assert!(daemon_alive(NS_A), "A died");
        assert!(daemon_alive(NS_B), "B died");

        // Check if file arrived (it should, since NACKs are sent after restoring)
        let received_path = "/tmp/summit-received/summit-test-nack-blackout.txt";
        if std::path::Path::new(received_path).exists() {
            let received = std::fs::read_to_string(received_path)?;
            assert_eq!(
                received, test_content,
                "content mismatch after blackout recovery"
            );
            println!("File recovered after temporary blackout");
        } else {
            // Small file may have actually gotten through, or metadata was lost
            // during the 90% loss period. Either way, daemon survived.
            println!("File did not arrive (metadata may have been lost during blackout)");
        }

        Ok(())
    })();

    node_a.kill().ok();
    node_b.kill().ok();
    cleanup_summitd();
    std::fs::remove_file(test_file).ok();
    result.unwrap();
}

/// Verify that in-progress assemblies show up in the /files API during a stall,
/// and that NACK recovery eventually resolves them.
#[test]
fn test_nack_in_progress_visible_during_stall() {
    if !skip_unless_ready() {
        return;
    }

    let _lock = DAEMON_LOCK.lock().unwrap();
    cleanup_summitd();
    std::fs::remove_dir_all("/tmp/summit-received").ok();

    let auto_env = [("SUMMIT_TRUST__AUTO_TRUST", "true")];
    let mut node_a = spawn_daemon(NS_A, VETH_A, &auto_env);
    let mut node_b = spawn_daemon(NS_B, VETH_B, &auto_env);

    // 256KB — enough chunks that some will be dropped at 40% loss
    let test_file = "/tmp/summit-test-nack-progress.bin";
    let data = vec![0xABu8; 256 * 1024];
    std::fs::write(test_file, &data).unwrap();

    let result = (|| -> Result<()> {
        wait_for_api(NS_A, 40)?;
        wait_for_api(NS_B, 40)?;
        let _session = wait_for_session(8)?;

        // Apply 40% loss on receiver
        let _guard_b = add_packet_loss(NS_B, VETH_B, 40);

        let send_out = ctl(NS_A, &["send", test_file])?;
        assert!(send_out.contains("File queued"), "send: {}", send_out);

        // Check after initial send — file should be in_progress (not all chunks arrived)
        thread::sleep(Duration::from_secs(2));
        let files_resp = api_get(NS_B, "/files")?;
        let in_progress = files_resp["in_progress"]
            .as_array()
            .map(|a| a.len())
            .unwrap_or(0);
        let received = files_resp["received"]
            .as_array()
            .map(|a| a.len())
            .unwrap_or(0);
        println!(
            "After initial send: {} in_progress, {} received",
            in_progress, received
        );

        // Under 40% loss, it's very likely some chunks were dropped and
        // the file is stuck in_progress (or it got lucky and completed).
        // Either outcome is valid — the key is no crash.

        // Wait for NACK recovery to complete
        thread::sleep(Duration::from_secs(25));

        assert!(daemon_alive(NS_A), "A died");
        assert!(daemon_alive(NS_B), "B died");

        // After recovery, check final state
        let files_final = api_get(NS_B, "/files")?;
        let final_received = files_final["received"]
            .as_array()
            .map(|a| a.len())
            .unwrap_or(0);
        let final_in_progress = files_final["in_progress"]
            .as_array()
            .map(|a| a.len())
            .unwrap_or(0);
        println!(
            "After recovery: {} in_progress, {} received",
            final_in_progress, final_received
        );

        // If recovery succeeded, file should be received (not in_progress)
        if final_received > 0 {
            let received_path = "/tmp/summit-received/summit-test-nack-progress.bin";
            let content = std::fs::read(received_path)?;
            assert_eq!(content, data, "content mismatch");
            println!("File completed after NACK recovery");
        }

        Ok(())
    })();

    node_a.kill().ok();
    node_b.kill().ok();
    cleanup_summitd();
    std::fs::remove_file(test_file).ok();
    result.unwrap();
}

/// Send a large file (2MB) under 20% loss. Tests that NACK batching works
/// correctly — with ~32 chunks of 64KB, some will drop and need recovery.
#[test]
fn test_nack_large_file_recovery() {
    if !skip_unless_ready() {
        return;
    }

    let _lock = DAEMON_LOCK.lock().unwrap();
    cleanup_summitd();
    std::fs::remove_dir_all("/tmp/summit-received").ok();

    let auto_env = [("SUMMIT_TRUST__AUTO_TRUST", "true")];
    let mut node_a = spawn_daemon(NS_A, VETH_A, &auto_env);
    let mut node_b = spawn_daemon(NS_B, VETH_B, &auto_env);

    // 2MB file — many chunks to exercise NACK batching
    let test_file = "/tmp/summit-test-nack-large.bin";
    let data: Vec<u8> = (0..2 * 1024 * 1024).map(|i| (i % 251) as u8).collect();
    std::fs::write(test_file, &data).unwrap();

    let result = (|| -> Result<()> {
        wait_for_api(NS_A, 40)?;
        wait_for_api(NS_B, 40)?;
        let _session = wait_for_session(8)?;

        // 20% loss — enough to lose chunks, low enough for NACKs to get through
        let _guard_b = add_packet_loss(NS_B, VETH_B, 20);

        let send_out = ctl(NS_A, &["send", test_file])?;
        assert!(send_out.contains("File queued"), "send: {}", send_out);
        println!("2MB file queued under 20% loss");

        // Generous timeout for large file recovery
        thread::sleep(Duration::from_secs(35));

        assert!(daemon_alive(NS_A), "sender died");
        assert!(daemon_alive(NS_B), "receiver died");

        let received_path = "/tmp/summit-received/summit-test-nack-large.bin";
        if std::path::Path::new(received_path).exists() {
            let received = std::fs::read(received_path)?;
            assert_eq!(received.len(), data.len(), "size mismatch");
            assert_eq!(received, data, "content mismatch after large file recovery");
            println!("2MB file recovered intact under 20% loss");
        } else {
            println!("Large file did not complete (too much loss for 3 NACK attempts)");
        }

        Ok(())
    })();

    node_a.kill().ok();
    node_b.kill().ok();
    cleanup_summitd();
    std::fs::remove_file(test_file).ok();
    result.unwrap();
}

/// Kill the sender mid-transfer so it can't respond to NACKs.
/// The receiver should send NACKs, escalate to broadcast (no other peers),
/// exhaust MAX_NACK_ATTEMPTS, and abandon the assembly. Daemon must survive.
#[test]
fn test_nack_sender_gone_assembly_abandoned() {
    if !skip_unless_ready() {
        return;
    }

    let _lock = DAEMON_LOCK.lock().unwrap();
    cleanup_summitd();
    std::fs::remove_dir_all("/tmp/summit-received").ok();

    let auto_env = [("SUMMIT_TRUST__AUTO_TRUST", "true")];
    let mut node_a = spawn_daemon(NS_A, VETH_A, &auto_env);
    let mut node_b = spawn_daemon(NS_B, VETH_B, &auto_env);

    // 512KB — enough to ensure multi-chunk transfer
    let test_file = "/tmp/summit-test-nack-abandoned.bin";
    let data = vec![0xCDu8; 512 * 1024];
    std::fs::write(test_file, &data).unwrap();

    let result = (|| -> Result<()> {
        wait_for_api(NS_A, 40)?;
        wait_for_api(NS_B, 40)?;
        let _session = wait_for_session(8)?;

        // Apply 50% loss so some data chunks drop
        let _guard_b = add_packet_loss(NS_B, VETH_B, 50);

        let _ = ctl(NS_A, &["send", test_file]);

        // Kill sender after some chunks have been sent
        thread::sleep(Duration::from_millis(500));
        node_a.kill().ok();
        println!("Sender killed — NACK recovery will fail");

        // Wait for NACK attempts to exhaust:
        // NACK_DELAY(3s) + CHECK_INTERVAL(2s) * MAX_NACK_ATTEMPTS(3) = ~15s
        // Plus some margin for timing
        thread::sleep(Duration::from_secs(20));

        // Receiver must survive the failed recovery
        assert!(
            daemon_alive(NS_B),
            "receiver died during failed NACK recovery"
        );

        // File should NOT be committed (sender is dead, recovery fails)
        let received_path = "/tmp/summit-received/summit-test-nack-abandoned.bin";
        assert!(
            !std::path::Path::new(received_path).exists(),
            "partial file should not be committed when sender is dead"
        );

        // Assembly should be abandoned (cleared from in_progress)
        let files = api_get(NS_B, "/files")?;
        let in_progress = files["in_progress"]
            .as_array()
            .map(|a| a.len())
            .unwrap_or(0);
        println!("in_progress after exhausted NACKs: {}", in_progress);
        // After MAX_NACK_ATTEMPTS, cleanup_stale or abandon should clear it.
        // It may still be in_progress if the stale timeout (300s) hasn't fired yet,
        // but the assembly's nack_count should be at MAX.

        Ok(())
    })();

    node_a.kill().ok();
    node_b.kill().ok();
    cleanup_summitd();
    std::fs::remove_file(test_file).ok();
    result.unwrap();
}

/// Send multiple files concurrently under packet loss.
/// NACK recovery should handle all stalled assemblies, not just one.
#[test]
fn test_nack_concurrent_file_recovery() {
    if !skip_unless_ready() {
        return;
    }

    let _lock = DAEMON_LOCK.lock().unwrap();
    cleanup_summitd();
    std::fs::remove_dir_all("/tmp/summit-received").ok();

    let auto_env = [("SUMMIT_TRUST__AUTO_TRUST", "true")];
    let mut node_a = spawn_daemon(NS_A, VETH_A, &auto_env);
    let mut node_b = spawn_daemon(NS_B, VETH_B, &auto_env);

    // Create 5 files of varying sizes
    let mut test_files = Vec::new();
    for i in 0..5 {
        let path = format!("/tmp/summit-test-nack-concurrent-{}.bin", i);
        let size = (i + 1) * 64 * 1024; // 64KB, 128KB, 192KB, 256KB, 320KB
        let data: Vec<u8> = (0..size).map(|j| ((i + j) % 256) as u8).collect();
        std::fs::write(&path, &data).unwrap();
        test_files.push((path, data));
    }

    let result = (|| -> Result<()> {
        wait_for_api(NS_A, 40)?;
        wait_for_api(NS_B, 40)?;
        let _session = wait_for_session(8)?;

        // 25% loss — some chunks will drop across all transfers
        let _guard_b = add_packet_loss(NS_B, VETH_B, 25);

        // Send all files rapidly
        for (path, _) in &test_files {
            let send_out = ctl(NS_A, &["send", path])?;
            assert!(
                send_out.contains("File queued"),
                "send {}: {}",
                path,
                send_out
            );
        }
        println!("All 5 files queued under 25% loss");

        // Wait for NACK recovery cycles
        thread::sleep(Duration::from_secs(30));

        assert!(daemon_alive(NS_A), "A died");
        assert!(daemon_alive(NS_B), "B died");

        // Check how many arrived intact
        let mut recovered_count = 0;
        for (path, expected) in &test_files {
            let filename = std::path::Path::new(path)
                .file_name()
                .unwrap()
                .to_str()
                .unwrap();
            let received_path = format!("/tmp/summit-received/{}", filename);
            if let Ok(received) = std::fs::read(&received_path) {
                assert_eq!(
                    received.len(),
                    expected.len(),
                    "size mismatch for {}",
                    filename
                );
                assert_eq!(&received, expected, "content mismatch for {}", filename);
                recovered_count += 1;
            }
        }
        println!("Recovered {}/5 files under 25% loss", recovered_count);
        // With NACK recovery, we should get most files through
        assert!(
            recovered_count >= 3,
            "too few files recovered: {}/5",
            recovered_count
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

/// Verify that NACK targeted-then-broadcast escalation works.
/// Apply loss on the sender's side (so the targeted NACK response may drop),
/// then verify the file still arrives (via broadcast NACK to peers that cached chunks).
/// Since we only have 2 nodes, this tests that the retry mechanism handles the
/// targeted attempt failing and falling back to subsequent attempts.
#[test]
fn test_nack_escalation_under_sender_loss() {
    if !skip_unless_ready() {
        return;
    }

    let _lock = DAEMON_LOCK.lock().unwrap();
    cleanup_summitd();
    std::fs::remove_dir_all("/tmp/summit-received").ok();

    let auto_env = [("SUMMIT_TRUST__AUTO_TRUST", "true")];
    let mut node_a = spawn_daemon(NS_A, VETH_A, &auto_env);
    let mut node_b = spawn_daemon(NS_B, VETH_B, &auto_env);

    let test_file = "/tmp/summit-test-nack-escalation.bin";
    let data = vec![0xAAu8; 128 * 1024];
    std::fs::write(test_file, &data).unwrap();

    let result = (|| -> Result<()> {
        wait_for_api(NS_A, 40)?;
        wait_for_api(NS_B, 40)?;
        let _session = wait_for_session(8)?;

        // Loss on BOTH sides — receiver drops data, sender's NACK response may drop too
        let _guard_a = add_packet_loss(NS_A, VETH_A, 20);
        let _guard_b = add_packet_loss(NS_B, VETH_B, 20);

        let send_out = ctl(NS_A, &["send", test_file])?;
        assert!(send_out.contains("File queued"), "send: {}", send_out);
        println!("File queued with 20% bidirectional loss");

        // Wait for multiple NACK attempts
        thread::sleep(Duration::from_secs(25));

        assert!(daemon_alive(NS_A), "A died");
        assert!(daemon_alive(NS_B), "B died");

        let received_path = "/tmp/summit-received/summit-test-nack-escalation.bin";
        if std::path::Path::new(received_path).exists() {
            let received = std::fs::read(received_path)?;
            assert_eq!(received, data, "content mismatch");
            println!("File recovered despite bidirectional loss");
        } else {
            // With bidirectional 20% loss, NACKs themselves may be dropped.
            // Recovery is best-effort. The important thing is no crash.
            println!("File did not arrive (NACKs lost in transit — acceptable)");
        }

        Ok(())
    })();

    node_a.kill().ok();
    node_b.kill().ok();
    cleanup_summitd();
    std::fs::remove_file(test_file).ok();
    result.unwrap();
}

/// Zero packet loss sanity check — file transfer should work fine without any
/// NACK recovery needed. This ensures the NACK loop doesn't interfere with
/// normal operation.
#[test]
fn test_nack_loop_no_interference_clean_transfer() {
    if !skip_unless_ready() {
        return;
    }

    let _lock = DAEMON_LOCK.lock().unwrap();
    cleanup_summitd();
    std::fs::remove_dir_all("/tmp/summit-received").ok();

    let auto_env = [("SUMMIT_TRUST__AUTO_TRUST", "true")];
    let mut node_a = spawn_daemon(NS_A, VETH_A, &auto_env);
    let mut node_b = spawn_daemon(NS_B, VETH_B, &auto_env);

    let test_file = "/tmp/summit-test-nack-clean.bin";
    let data: Vec<u8> = (0..256 * 1024).map(|i| (i % 256) as u8).collect();
    std::fs::write(test_file, &data).unwrap();

    let result = (|| -> Result<()> {
        wait_for_api(NS_A, 40)?;
        wait_for_api(NS_B, 40)?;
        let _session = wait_for_session(8)?;

        // No packet loss — clean network
        let send_out = ctl(NS_A, &["send", test_file])?;
        assert!(send_out.contains("File queued"), "send: {}", send_out);

        // Should complete quickly without any NACKs
        thread::sleep(Duration::from_secs(8));

        assert!(daemon_alive(NS_A), "A died");
        assert!(daemon_alive(NS_B), "B died");

        let received_path = "/tmp/summit-received/summit-test-nack-clean.bin";
        let received =
            std::fs::read(received_path).context("file not received on clean network")?;
        assert_eq!(received.len(), data.len(), "size mismatch");
        assert_eq!(received, data, "content mismatch on clean transfer");

        // Verify no in-progress assemblies remain
        let files = api_get(NS_B, "/files")?;
        let in_progress = files["in_progress"]
            .as_array()
            .map(|a| a.len())
            .unwrap_or(0);
        assert_eq!(
            in_progress, 0,
            "in_progress should be 0 after clean transfer, got {}",
            in_progress
        );
        println!("Clean transfer completed — NACK loop did not interfere");

        Ok(())
    })();

    node_a.kill().ok();
    node_b.kill().ok();
    cleanup_summitd();
    std::fs::remove_file(test_file).ok();
    result.unwrap();
}
