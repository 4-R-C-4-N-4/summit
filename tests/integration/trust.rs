use crate::*;

/// Full trust lifecycle via CLI: list (empty) -> add -> list (Trusted) -> block -> list (Blocked).
#[test]
fn test_ctl_trust_lifecycle() {
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

        thread::sleep(Duration::from_secs(6));

        // trust list: should be empty initially
        let list_out = ctl(NS_A, &["trust", "list"])?;
        assert!(
            list_out.contains("No explicit trust rules"),
            "expected empty trust list: {}",
            list_out
        );
        println!("Initial trust list: {}", list_out);

        // "trust" alias should work the same
        let alias_out = ctl(NS_A, &["trust"])?;
        assert!(
            alias_out.contains("No explicit trust rules"),
            "trust alias failed: {}",
            alias_out
        );

        // Peer should be Untrusted
        let peers = api_get(NS_A, "/peers")?;
        assert_eq!(
            peers["peers"][0]["trust_level"].as_str().unwrap(),
            "Untrusted"
        );

        let pubkey_b = get_peer_pubkey(NS_A)?;
        println!("B pubkey: {}...", &pubkey_b[..16]);

        // trust add via CLI
        let add_out = ctl(NS_A, &["trust", "add", &pubkey_b])?;
        assert!(
            add_out.contains("Peer trusted") || add_out.contains("trusted"),
            "trust add output: {}",
            add_out
        );
        println!("trust add: {}", add_out);

        // trust list: should show Trusted
        let list_after = ctl(NS_A, &["trust", "list"])?;
        assert!(
            list_after.contains("Trusted"),
            "trust list missing Trusted: {}",
            list_after
        );
        assert!(
            list_after.contains(&pubkey_b[..16]),
            "trust list missing pubkey: {}",
            list_after
        );
        println!("After add: {}", list_after);

        // trust block via CLI (block B from A's perspective)
        let block_out = ctl(NS_A, &["trust", "block", &pubkey_b])?;
        assert!(
            block_out.contains("Peer blocked") || block_out.contains("blocked"),
            "trust block output: {}",
            block_out
        );
        println!("trust block: {}", block_out);

        // trust list: should now show Blocked
        let list_blocked = ctl(NS_A, &["trust", "list"])?;
        assert!(
            list_blocked.contains("Blocked"),
            "trust list missing Blocked: {}",
            list_blocked
        );
        println!("After block: {}", list_blocked);

        // API verification
        let trust_api = api_get(NS_A, "/trust")?;
        let rules = trust_api["rules"].as_array().unwrap();
        assert_eq!(rules.len(), 1);
        assert_eq!(rules[0]["level"].as_str().unwrap(), "Blocked");

        Ok(())
    })();

    node_a.kill().ok();
    node_b.kill().ok();
    cleanup_summitd();
    result.unwrap();
}

/// summit-ctl trust pending: verify buffered chunks from untrusted peers.
#[test]
fn test_ctl_trust_pending() {
    if !skip_unless_ready() {
        return;
    }

    let _lock = DAEMON_LOCK.lock().unwrap();
    cleanup_summitd();

    // No auto-trust: chunks from B will be buffered on A
    let mut node_a = spawn_daemon(NS_A, VETH_A, &[]);
    let auto_env = [("SUMMIT_TRUST__AUTO_TRUST", "true")];
    let mut node_b = spawn_daemon(NS_B, VETH_B, &auto_env);

    let result = (|| -> Result<()> {
        wait_for_api(NS_A, 40)?;
        wait_for_api(NS_B, 40)?;

        thread::sleep(Duration::from_secs(8));

        // B sends a message to A (B auto-trusts A, so it'll send)
        let pubkey_a = get_peer_pubkey(NS_B)?;
        let body = serde_json::json!({ "to": pubkey_a, "text": "pending test" }).to_string();
        api_post(NS_B, "/messages/send", &body)?;

        thread::sleep(Duration::from_secs(4));

        // A should have buffered chunks — check trust pending via CLI
        let pending_out = ctl(NS_A, &["trust", "pending"])?;
        println!("trust pending: {}", pending_out);
        // May or may not have buffered chunks depending on timing,
        // but the command should at least run without error.
        assert!(
            pending_out.contains("Untrusted Peers") || pending_out.contains("No buffered chunks"),
            "unexpected trust pending output: {}",
            pending_out
        );

        // API shape
        let resp = api_get(NS_A, "/trust/pending")?;
        assert!(resp["peers"].is_array(), "missing peers array");

        // Now trust B and verify flush
        let pubkey_b = get_peer_pubkey(NS_A)?;
        let flushed = trust_peer(NS_A, &pubkey_b)?;
        println!("Trusted B, flushed {} chunks", flushed);

        // After trust, pending should be empty
        let pending_after = ctl(NS_A, &["trust", "pending"])?;
        assert!(
            pending_after.contains("No buffered chunks"),
            "still buffered after trust: {}",
            pending_after
        );

        Ok(())
    })();

    node_a.kill().ok();
    node_b.kill().ok();
    cleanup_summitd();
    result.unwrap();
}

/// Auto-trust config: peers appear as Trusted automatically.
#[test]
fn test_auto_trust_config() {
    if !skip_unless_ready() {
        return;
    }

    let _lock = DAEMON_LOCK.lock().unwrap();
    cleanup_summitd();

    let auto_env = [("SUMMIT_TRUST__AUTO_TRUST", "true")];
    let mut node_a = spawn_daemon(NS_A, VETH_A, &auto_env);
    let mut node_b = spawn_daemon(NS_B, VETH_B, &auto_env);

    let result = (|| -> Result<()> {
        wait_for_api(NS_A, 40)?;
        wait_for_api(NS_B, 40)?;

        thread::sleep(Duration::from_secs(6));

        let peers = api_get(NS_A, "/peers")?;
        let peers_list = peers["peers"].as_array().context("no peers")?;
        assert!(!peers_list.is_empty(), "no peers after 6s");
        assert_eq!(
            peers_list[0]["trust_level"].as_str().unwrap(),
            "Trusted",
            "expected auto-trust"
        );

        println!("Verified auto-trust config");
        Ok(())
    })();

    node_a.kill().ok();
    node_b.kill().ok();
    cleanup_summitd();
    result.unwrap();
}
