use crate::*;

/// Verify sessions establish and summit-ctl sessions inspect works.
#[test]
fn test_ctl_sessions_inspect() {
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

        let session_id = wait_for_session(8)?;
        println!("Session: {}...", &session_id[..16]);

        // API shape
        let status = api_get(NS_A, "/status")?;
        let session = &status["sessions"].as_array().unwrap()[0];
        assert!(session["session_id"].is_string(), "missing session_id");
        assert!(session["peer_pubkey"].is_string(), "missing peer_pubkey");
        assert!(session["contract"].is_string(), "missing contract");
        assert!(session["chunk_port"].is_number(), "missing chunk_port");
        assert!(
            session["established_secs"].is_number(),
            "missing established_secs"
        );
        assert!(session["trust_level"].is_string(), "missing trust_level");

        // summit-ctl sessions inspect
        let out = ctl(NS_A, &["sessions", "inspect", &session_id])?;
        assert!(out.contains("Session Details"), "header missing: {}", out);
        assert!(out.contains("Peer"), "Peer line missing: {}", out);
        assert!(out.contains("Pubkey"), "Pubkey line missing: {}", out);
        assert!(out.contains("Contract"), "Contract line missing: {}", out);
        assert!(out.contains("Trust"), "Trust line missing: {}", out);
        println!("{}", out);

        Ok(())
    })();

    node_a.kill().ok();
    node_b.kill().ok();
    cleanup_summitd();
    result.unwrap();
}

/// summit-ctl sessions drop: drop an active session and verify it's gone.
#[test]
fn test_ctl_sessions_drop() {
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

        let session_id = wait_for_session(8)?;
        println!("Dropping session: {}...", &session_id[..16]);

        // Drop via CLI
        let out = ctl(NS_A, &["sessions", "drop", &session_id])?;
        assert!(
            out.contains("Session dropped") || out.contains("dropped"),
            "drop output unexpected: {}",
            out
        );
        println!("{}", out);

        // Verify it's gone
        let status = api_get(NS_A, "/status")?;
        let sessions = status["sessions"].as_array().unwrap();
        let still_exists = sessions
            .iter()
            .any(|s| s["session_id"].as_str() == Some(&session_id));
        assert!(!still_exists, "session still present after drop");

        // Dropping a non-existent session should report not found
        let out2 = ctl(
            NS_A,
            &[
                "sessions",
                "drop",
                "0000000000000000000000000000000000000000000000000000000000000000",
            ],
        )?;
        assert!(
            out2.contains("not found") || out2.contains("Session not found"),
            "expected 'not found' for bogus session: {}",
            out2
        );

        Ok(())
    })();

    node_a.kill().ok();
    node_b.kill().ok();
    cleanup_summitd();
    result.unwrap();
}
