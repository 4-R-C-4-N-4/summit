use crate::*;

/// Disabling messaging reduces announced service_count.
#[test]
fn test_service_config_disable_messaging() {
    if !skip_unless_ready() {
        return;
    }

    let _lock = DAEMON_LOCK.lock().unwrap();
    cleanup_summitd();

    let a_env = [("SUMMIT_SERVICES__MESSAGING", "false")];
    let mut node_a = spawn_daemon(NS_A, VETH_A, &a_env);
    let mut node_b = spawn_daemon(NS_B, VETH_B, &[]);

    let result = (|| -> Result<()> {
        wait_for_api(NS_A, 40)?;
        wait_for_api(NS_B, 40)?;

        thread::sleep(Duration::from_secs(6));

        // B sees A with only 1 service (file_transfer)
        let peers_b = api_get(NS_B, "/peers")?;
        let peers_list = peers_b["peers"].as_array().context("no peers on B")?;
        assert!(!peers_list.is_empty(), "B has no peers");

        let svc_count = peers_list[0]["service_count"].as_u64().unwrap_or(99);
        assert_eq!(
            svc_count, 1,
            "expected 1 service (messaging disabled), got {}",
            svc_count
        );
        assert!(peers_list[0]["is_complete"].as_bool().unwrap_or(false));

        // A sees B with 2 services (default config)
        let peers_a = api_get(NS_A, "/peers")?;
        let peers_list_a = peers_a["peers"].as_array().context("no peers on A")?;
        let svc_count_b = peers_list_a[0]["service_count"].as_u64().unwrap_or(0);
        assert!(svc_count_b >= 2, "expected >=2 from B, got {}", svc_count_b);

        println!("Verified service disable");
        Ok(())
    })();

    node_a.kill().ok();
    node_b.kill().ok();
    cleanup_summitd();
    result.unwrap();
}

/// Enabling compute service shows in service list and discovery.
#[test]
fn test_service_config_enable_compute() {
    if !skip_unless_ready() {
        return;
    }

    let _lock = DAEMON_LOCK.lock().unwrap();
    cleanup_summitd();

    let env = [("SUMMIT_SERVICES__COMPUTE", "true")];
    let mut node_a = spawn_daemon(NS_A, VETH_A, &env);
    let mut node_b = spawn_daemon(NS_B, VETH_B, &env);

    let result = (|| -> Result<()> {
        wait_for_api(NS_A, 40)?;
        wait_for_api(NS_B, 40)?;

        // Verify compute shows as enabled
        let svc = api_get(NS_A, "/services")?;
        let svc_list = svc["services"].as_array().context("no services")?;
        let compute = svc_list
            .iter()
            .find(|s| s["name"].as_str() == Some("compute"));
        assert!(compute.is_some(), "compute not in services list");
        assert!(compute.unwrap()["enabled"].as_bool().unwrap_or(false));

        let out = ctl(NS_A, &["services"])?;
        assert!(out.contains("compute"), "compute missing from CLI: {}", out);
        assert!(
            out.contains("enabled"),
            "compute not enabled in CLI: {}",
            out
        );
        println!("{}", out);

        // After discovery, peer should show 3 services (file_transfer + messaging + compute)
        thread::sleep(Duration::from_secs(6));
        let peers = api_get(NS_A, "/peers")?;
        let peers_list = peers["peers"].as_array().context("no peers")?;
        if !peers_list.is_empty() {
            let svc_count = peers_list[0]["service_count"].as_u64().unwrap_or(0);
            assert!(
                svc_count >= 3,
                "expected >=3 services with compute, got {}",
                svc_count
            );
        }

        Ok(())
    })();

    node_a.kill().ok();
    node_b.kill().ok();
    cleanup_summitd();
    result.unwrap();
}
