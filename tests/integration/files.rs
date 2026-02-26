use crate::*;

/// End-to-end file transfer (broadcast): A sends a file, B receives and reassembles.
#[test]
fn test_file_transfer_broadcast() {
    if !skip_unless_ready() {
        return;
    }

    let _lock = DAEMON_LOCK.lock().unwrap();
    cleanup_summitd();
    std::fs::remove_dir_all("/tmp/summit-received").ok();

    let auto_env = [("SUMMIT_TRUST__AUTO_TRUST", "true")];
    let mut node_a = spawn_daemon(NS_A, VETH_A, &auto_env);
    let mut node_b = spawn_daemon(NS_B, VETH_B, &auto_env);

    let test_content = "Summit integration test — broadcast file transfer";
    let test_file = "/tmp/summit-test-broadcast.txt";
    std::fs::write(test_file, test_content).unwrap();

    let result = (|| -> Result<()> {
        wait_for_api(NS_A, 40)?;
        wait_for_api(NS_B, 40)?;

        thread::sleep(Duration::from_secs(8));
        assert!(!api_get(NS_A, "/status")?["sessions"]
            .as_array()
            .unwrap()
            .is_empty());

        // Send file via CLI (broadcast)
        let send_out = ctl(NS_A, &["send", test_file])?;
        assert!(
            send_out.contains("File queued"),
            "send output: {}",
            send_out
        );
        assert!(
            send_out.contains("broadcast"),
            "missing broadcast target: {}",
            send_out
        );
        assert!(
            send_out.contains("Chunks"),
            "missing chunks info: {}",
            send_out
        );
        println!("send: {}", send_out);

        thread::sleep(Duration::from_secs(8));

        // summit-ctl files on B
        let files_out = ctl(NS_B, &["files"])?;
        assert!(
            files_out.contains("summit-test-broadcast.txt"),
            "file not received: {}",
            files_out
        );
        println!("files: {}", files_out);

        // Verify file content
        let received = std::fs::read_to_string("/tmp/summit-received/summit-test-broadcast.txt")
            .context("received file not found")?;
        assert_eq!(received, test_content, "content mismatch");

        Ok(())
    })();

    node_a.kill().ok();
    node_b.kill().ok();
    cleanup_summitd();
    std::fs::remove_file(test_file).ok();
    result.unwrap();
}

/// File transfer with --peer targeting: send to a specific peer by pubkey.
#[test]
fn test_file_transfer_targeted_peer() {
    if !skip_unless_ready() {
        return;
    }

    let _lock = DAEMON_LOCK.lock().unwrap();
    cleanup_summitd();
    std::fs::remove_dir_all("/tmp/summit-received").ok();

    let auto_env = [("SUMMIT_TRUST__AUTO_TRUST", "true")];
    let mut node_a = spawn_daemon(NS_A, VETH_A, &auto_env);
    let mut node_b = spawn_daemon(NS_B, VETH_B, &auto_env);

    let test_content = "targeted peer file transfer content";
    let test_file = "/tmp/summit-test-peer-target.txt";
    std::fs::write(test_file, test_content).unwrap();

    let result = (|| -> Result<()> {
        wait_for_api(NS_A, 40)?;
        wait_for_api(NS_B, 40)?;

        thread::sleep(Duration::from_secs(8));

        let pubkey_b = get_peer_pubkey(NS_A)?;
        println!("Sending to peer {}...", &pubkey_b[..16]);

        // Send file with --peer targeting
        let send_out = ctl(NS_A, &["send", test_file, "--peer", &pubkey_b])?;
        assert!(
            send_out.contains("File queued"),
            "send output: {}",
            send_out
        );
        assert!(
            send_out.contains("to peer"),
            "missing peer target: {}",
            send_out
        );
        println!("send --peer: {}", send_out);

        thread::sleep(Duration::from_secs(8));

        let files_out = ctl(NS_B, &["files"])?;
        assert!(
            files_out.contains("summit-test-peer-target.txt"),
            "targeted file not received: {}",
            files_out
        );

        let received = std::fs::read_to_string("/tmp/summit-received/summit-test-peer-target.txt")
            .context("received file not found")?;
        assert_eq!(received, test_content, "content mismatch");

        Ok(())
    })();

    node_a.kill().ok();
    node_b.kill().ok();
    cleanup_summitd();
    std::fs::remove_file(test_file).ok();
    result.unwrap();
}

/// File transfer with --session targeting: send to a specific session by ID.
#[test]
fn test_file_transfer_targeted_session() {
    if !skip_unless_ready() {
        return;
    }

    let _lock = DAEMON_LOCK.lock().unwrap();
    cleanup_summitd();
    std::fs::remove_dir_all("/tmp/summit-received").ok();

    let auto_env = [("SUMMIT_TRUST__AUTO_TRUST", "true")];
    let mut node_a = spawn_daemon(NS_A, VETH_A, &auto_env);
    let mut node_b = spawn_daemon(NS_B, VETH_B, &auto_env);

    let test_content = "session-targeted file transfer content";
    let test_file = "/tmp/summit-test-session-target.txt";
    std::fs::write(test_file, test_content).unwrap();

    let result = (|| -> Result<()> {
        wait_for_api(NS_A, 40)?;
        wait_for_api(NS_B, 40)?;

        let session_id = wait_for_session(8)?;
        println!("Sending to session {}...", &session_id[..16]);

        // Send file with --session targeting
        let send_out = ctl(NS_A, &["send", test_file, "--session", &session_id])?;
        assert!(
            send_out.contains("File queued"),
            "send output: {}",
            send_out
        );
        assert!(
            send_out.contains("to session"),
            "missing session target: {}",
            send_out
        );
        println!("send --session: {}", send_out);

        thread::sleep(Duration::from_secs(8));

        let files_out = ctl(NS_B, &["files"])?;
        assert!(
            files_out.contains("summit-test-session-target.txt"),
            "session-targeted file not received: {}",
            files_out
        );

        let received =
            std::fs::read_to_string("/tmp/summit-received/summit-test-session-target.txt")
                .context("received file not found")?;
        assert_eq!(received, test_content, "content mismatch");

        Ok(())
    })();

    node_a.kill().ok();
    node_b.kill().ok();
    cleanup_summitd();
    std::fs::remove_file(test_file).ok();
    result.unwrap();
}

/// summit-ctl files: verify output when no files have been received.
#[test]
fn test_ctl_files_empty() {
    if !skip_unless_ready() {
        return;
    }

    let _lock = DAEMON_LOCK.lock().unwrap();
    cleanup_summitd();

    let mut node_a = spawn_daemon(NS_A, VETH_A, &[]);

    let result = (|| -> Result<()> {
        wait_for_api(NS_A, 40)?;

        let out = ctl(NS_A, &["files"])?;
        assert!(
            out.contains("No files received") || out.contains("Received Files"),
            "unexpected files output: {}",
            out
        );
        println!("files (empty): {}", out);

        Ok(())
    })();

    node_a.kill().ok();
    cleanup_summitd();
    result.unwrap();
}
