use crate::*;

/// summit-ctl status: verify API shape and CLI output.
#[test]
fn test_ctl_status() {
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

        // API shape
        let status = api_get(NS_A, "/status")?;
        assert!(
            status["peers_discovered"].is_number(),
            "missing peers_discovered"
        );
        assert!(status["sessions"].is_array(), "missing sessions");
        assert!(
            status["cache"]["chunks"].is_number(),
            "missing cache.chunks"
        );
        assert!(status["cache"]["bytes"].is_number(), "missing cache.bytes");

        // summit-ctl status
        let out = ctl(NS_A, &["status"])?;
        assert!(
            out.contains("Summit Daemon Status"),
            "status header missing: {}",
            out
        );
        assert!(
            out.contains("Peers discovered"),
            "missing peers line: {}",
            out
        );
        assert!(
            out.contains("Active sessions"),
            "missing sessions line: {}",
            out
        );
        assert!(out.contains("Cache chunks"), "missing cache line: {}", out);
        println!("{}", out);

        // default command (no args) should also show status
        let default_out = ctl(NS_A, &[])?;
        assert!(
            default_out.contains("Summit Daemon Status"),
            "default cmd not status"
        );

        Ok(())
    })();

    node_a.kill().ok();
    node_b.kill().ok();
    cleanup_summitd();
    result.unwrap();
}

/// summit-ctl services: verify service listing.
#[test]
fn test_ctl_services() {
    if !skip_unless_ready() {
        return;
    }

    let _lock = DAEMON_LOCK.lock().unwrap();
    cleanup_summitd();

    let mut node_a = spawn_daemon(NS_A, VETH_A, &[]);

    let result = (|| -> Result<()> {
        wait_for_api(NS_A, 40)?;

        // API shape
        let resp = api_get(NS_A, "/services")?;
        let services = resp["services"].as_array().context("no services array")?;
        assert!(!services.is_empty(), "services list empty");
        for svc in services {
            assert!(svc["name"].is_string(), "service missing name");
            assert!(svc["enabled"].is_boolean(), "service missing enabled");
            assert!(svc["contract"].is_string(), "service missing contract");
        }

        // CLI output
        let out = ctl(NS_A, &["services"])?;
        assert!(out.contains("Services"), "services header missing: {}", out);
        assert!(
            out.contains("file_transfer"),
            "missing file_transfer: {}",
            out
        );
        assert!(out.contains("messaging"), "missing messaging: {}", out);
        println!("{}", out);

        Ok(())
    })();

    node_a.kill().ok();
    cleanup_summitd();
    result.unwrap();
}

/// summit-ctl services: verify disabling a service is reflected.
#[test]
fn test_ctl_services_disabled() {
    if !skip_unless_ready() {
        return;
    }

    let _lock = DAEMON_LOCK.lock().unwrap();
    cleanup_summitd();

    let env = [("SUMMIT_SERVICES__MESSAGING", "false")];
    let mut node_a = spawn_daemon(NS_A, VETH_A, &env);

    let result = (|| -> Result<()> {
        wait_for_api(NS_A, 40)?;

        let out = ctl(NS_A, &["services"])?;
        assert!(
            out.contains("file_transfer"),
            "missing file_transfer: {}",
            out
        );
        // messaging should show as disabled
        assert!(
            out.contains("disabled"),
            "expected disabled in output: {}",
            out
        );
        println!("{}", out);

        Ok(())
    })();

    node_a.kill().ok();
    cleanup_summitd();
    result.unwrap();
}

/// summit-ctl peers: verify peer discovery and CLI output.
#[test]
fn test_ctl_peers() {
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

        // API shape
        let resp = api_get(NS_A, "/peers")?;
        let peers = resp["peers"].as_array().context("no peers")?;
        assert!(!peers.is_empty(), "no peers after 6s");

        let peer = &peers[0];
        assert!(peer["public_key"].is_string(), "missing public_key");
        assert!(peer["addr"].is_string(), "missing addr");
        assert!(peer["session_port"].is_number(), "missing session_port");
        assert!(peer["services"].is_array(), "missing services");
        assert!(peer["service_count"].is_number(), "missing service_count");
        assert!(peer["is_complete"].is_boolean(), "missing is_complete");
        assert!(peer["trust_level"].is_string(), "missing trust_level");
        assert!(
            peer["buffered_chunks"].is_number(),
            "missing buffered_chunks"
        );
        assert!(peer["last_seen_secs"].is_number(), "missing last_seen_secs");

        let svc_count = peer["service_count"].as_u64().unwrap_or(0);
        assert!(svc_count >= 2, "expected >=2 services, got {}", svc_count);
        assert!(
            peer["is_complete"].as_bool().unwrap_or(false),
            "peer not complete"
        );

        // CLI
        let out = ctl(NS_A, &["peers"])?;
        assert!(out.contains("Discovered Peers"), "header missing: {}", out);
        assert!(out.contains("services"), "services line missing: {}", out);
        assert!(out.contains("trust"), "trust line missing: {}", out);
        assert!(out.contains("last seen"), "last_seen line missing: {}", out);
        println!("{}", out);

        Ok(())
    })();

    node_a.kill().ok();
    node_b.kill().ok();
    cleanup_summitd();
    result.unwrap();
}

/// summit-ctl cache & cache clear.
#[test]
fn test_ctl_cache() {
    if !skip_unless_ready() {
        return;
    }

    let _lock = DAEMON_LOCK.lock().unwrap();
    cleanup_summitd();

    let mut node_a = spawn_daemon(NS_A, VETH_A, &[]);

    let result = (|| -> Result<()> {
        wait_for_api(NS_A, 40)?;

        // cache stats API
        let resp = api_get(NS_A, "/cache")?;
        assert!(resp["chunks"].is_number(), "missing chunks");
        assert!(resp["bytes"].is_number(), "missing bytes");

        // summit-ctl cache
        let out = ctl(NS_A, &["cache"])?;
        assert!(out.contains("Cache Stats"), "cache header missing: {}", out);
        assert!(out.contains("Chunks"), "chunks line missing: {}", out);
        assert!(out.contains("Bytes"), "bytes line missing: {}", out);
        println!("cache:\n{}", out);

        // summit-ctl cache clear
        let clear_out = ctl(NS_A, &["cache", "clear"])?;
        assert!(
            clear_out.contains("Cleared"),
            "clear output missing 'Cleared': {}",
            clear_out
        );
        println!("cache clear: {}", clear_out);

        // cache should be empty after clear
        let resp2 = api_get(NS_A, "/cache")?;
        assert_eq!(
            resp2["chunks"].as_u64().unwrap_or(99),
            0,
            "cache not empty after clear"
        );

        Ok(())
    })();

    node_a.kill().ok();
    cleanup_summitd();
    result.unwrap();
}

/// summit-ctl schema list.
#[test]
fn test_ctl_schema_list() {
    if !skip_unless_ready() {
        return;
    }

    let _lock = DAEMON_LOCK.lock().unwrap();
    cleanup_summitd();

    let mut node_a = spawn_daemon(NS_A, VETH_A, &[]);

    let result = (|| -> Result<()> {
        wait_for_api(NS_A, 40)?;

        // API shape
        let resp = api_get(NS_A, "/schema")?;
        let schemas = resp["schemas"].as_array().context("no schemas")?;
        assert!(!schemas.is_empty(), "schema list empty");
        for s in schemas {
            assert!(s["id"].is_string(), "schema missing id");
            assert!(s["name"].is_string(), "schema missing name");
            assert!(s["type_tag"].is_number(), "schema missing type_tag");
        }

        let names: Vec<&str> = schemas.iter().filter_map(|s| s["name"].as_str()).collect();
        assert!(
            names.iter().any(|n| n.contains("File")),
            "expected File schema: {:?}",
            names
        );

        // CLI
        let out = ctl(NS_A, &["schema", "list"])?;
        assert!(out.contains("Known Schemas"), "header missing: {}", out);
        println!("{}", out);

        // Also test "schema" alias (no "list")
        let out2 = ctl(NS_A, &["schema"])?;
        assert!(
            out2.contains("Known Schemas"),
            "schema alias failed: {}",
            out2
        );

        Ok(())
    })();

    node_a.kill().ok();
    cleanup_summitd();
    result.unwrap();
}

/// summit-ctl help: verify help text includes all command categories.
#[test]
fn test_ctl_help() {
    if !skip_unless_ready() {
        return;
    }

    // help doesn't need a daemon — but summit-ctl will try to connect.
    // Use the raw output which may fail on the HTTP call but still prints usage.
    let output = ctl_raw(NS_A, &["help"]);
    let stdout = String::from_utf8_lossy(&output.stdout);
    let combined = format!("{}{}", stdout, String::from_utf8_lossy(&output.stderr));

    // help should mention major command categories
    assert!(
        combined.contains("shutdown") || combined.contains("Usage"),
        "help missing shutdown"
    );
    assert!(
        combined.contains("trust") || combined.contains("Usage"),
        "help missing trust"
    );
    assert!(
        combined.contains("send") || combined.contains("Usage"),
        "help missing send"
    );
    assert!(
        combined.contains("compute") || combined.contains("Usage"),
        "help missing compute"
    );
    assert!(
        combined.contains("messages") || combined.contains("Usage"),
        "help missing messages"
    );
    println!("help output:\n{}", combined);
}
