use crate::*;

/// Simple compute task: submit via API, verify on both sides, check CLI.
#[test]
fn test_compute_task_simple() {
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

        thread::sleep(Duration::from_secs(8));
        assert!(!api_get(NS_A, "/status")?["sessions"]
            .as_array()
            .unwrap()
            .is_empty());

        let pubkey_b = get_peer_pubkey(NS_A)?;
        let pubkey_a = get_peer_pubkey(NS_B)?;

        // Submit task via API
        let body = serde_json::json!({
            "to": pubkey_b,
            "payload": { "op": "echo", "input": "hello from A" }
        })
        .to_string();

        let resp = api_post(NS_A, "/compute/submit", &body)?;
        let task_id = resp["task_id"]
            .as_str()
            .context("missing task_id")?
            .to_string();
        assert!(resp["timestamp"].is_number(), "missing timestamp");
        println!("Task submitted: {}...", &task_id[..16]);

        // A's local store should have it immediately
        let tasks_a = api_get(NS_A, &format!("/compute/tasks/{}", pubkey_b))?;
        let a_list = tasks_a["tasks"].as_array().context("no tasks on A")?;
        assert!(!a_list.is_empty(), "task not in A's store");
        assert_eq!(a_list[0]["task_id"].as_str().unwrap(), task_id);
        assert_eq!(a_list[0]["status"].as_str().unwrap(), "Queued");

        thread::sleep(Duration::from_secs(4));

        // B should have received it
        let tasks_b = api_get(NS_B, &format!("/compute/tasks/{}", pubkey_a))?;
        let b_list = tasks_b["tasks"].as_array().context("no tasks on B")?;
        assert!(!b_list.is_empty(), "task not received on B");
        assert_eq!(b_list[0]["task_id"].as_str().unwrap(), task_id);
        assert_eq!(b_list[0]["status"].as_str().unwrap(), "Queued");

        // summit-ctl compute tasks <peer>
        let ctl_tasks = ctl(NS_B, &["compute", "tasks", &pubkey_a])?;
        assert!(
            ctl_tasks.contains(&task_id[..16]),
            "CLI missing task_id: {}",
            ctl_tasks
        );
        println!("compute tasks:\n{}", ctl_tasks);

        Ok(())
    })();

    node_a.kill().ok();
    node_b.kill().ok();
    cleanup_summitd();
    result.unwrap();
}

/// summit-ctl compute submit via CLI (JSON payload).
#[test]
fn test_ctl_compute_submit_json() {
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

        thread::sleep(Duration::from_secs(8));
        assert!(!api_get(NS_A, "/status")?["sessions"]
            .as_array()
            .unwrap()
            .is_empty());

        let pubkey_b = get_peer_pubkey(NS_A)?;

        // Submit via CLI with JSON payload
        let out = ctl(
            NS_A,
            &[
                "compute",
                "submit",
                &pubkey_b,
                r#"{"op":"echo","input":"cli json test"}"#,
            ],
        )?;
        assert!(out.contains("Compute task submitted"), "output: {}", out);
        assert!(out.contains("Task ID"), "missing Task ID: {}", out);
        assert!(out.contains("Timestamp"), "missing Timestamp: {}", out);
        println!("compute submit json:\n{}", out);

        Ok(())
    })();

    node_a.kill().ok();
    node_b.kill().ok();
    cleanup_summitd();
    result.unwrap();
}

/// summit-ctl compute submit via CLI (-- shell command syntax).
#[test]
fn test_ctl_compute_submit_shell() {
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

        thread::sleep(Duration::from_secs(8));
        assert!(!api_get(NS_A, "/status")?["sessions"]
            .as_array()
            .unwrap()
            .is_empty());

        let pubkey_b = get_peer_pubkey(NS_A)?;

        // Submit via CLI with -- shell command syntax
        let out = ctl(NS_A, &["compute", "submit", &pubkey_b, "--", "uname", "-a"])?;
        assert!(
            out.contains("Compute task submitted") || out.contains("Task ID"),
            "output: {}",
            out
        );
        println!("compute submit shell:\n{}", out);

        // Verify it's stored on A
        let tasks = api_get(NS_A, &format!("/compute/tasks/{}", pubkey_b))?;
        let task_list = tasks["tasks"].as_array().context("no tasks")?;
        assert!(!task_list.is_empty(), "shell task not stored");
        println!("Shell task confirmed in store");

        Ok(())
    })();

    node_a.kill().ok();
    node_b.kill().ok();
    cleanup_summitd();
    result.unwrap();
}

/// summit-ctl compute tasks (all tasks, no peer filter).
#[test]
fn test_ctl_compute_tasks_all() {
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

        thread::sleep(Duration::from_secs(8));

        let pubkey_b = get_peer_pubkey(NS_A)?;

        // Submit two tasks
        for i in 1..=2 {
            let body = serde_json::json!({
                "to": pubkey_b,
                "payload": { "op": "echo", "input": format!("task {}", i) }
            })
            .to_string();
            api_post(NS_A, "/compute/submit", &body)?;
            thread::sleep(Duration::from_millis(50));
        }

        // summit-ctl compute tasks (all)
        let out = ctl(NS_A, &["compute", "tasks"])?;
        assert!(
            out.contains("All Compute Tasks") || out.contains("No compute tasks"),
            "unexpected all tasks output: {}",
            out
        );
        println!("compute tasks (all):\n{}", out);

        // API shape
        let resp = api_get(NS_A, "/compute/tasks")?;
        assert!(resp["tasks"].is_array(), "missing tasks array");
        let all_tasks = resp["tasks"].as_array().unwrap();
        assert!(
            all_tasks.len() >= 2,
            "expected >=2 tasks, got {}",
            all_tasks.len()
        );

        Ok(())
    })();

    node_a.kill().ok();
    node_b.kill().ok();
    cleanup_summitd();
    result.unwrap();
}

/// Complex compute task: deeply nested JSON survives round-trip intact.
#[test]
fn test_compute_task_complex_payload() {
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

        thread::sleep(Duration::from_secs(8));
        assert!(!api_get(NS_A, "/status")?["sessions"]
            .as_array()
            .unwrap()
            .is_empty());

        let pubkey_b = get_peer_pubkey(NS_A)?;
        let pubkey_a = get_peer_pubkey(NS_B)?;

        // Submit a complex nested pipeline task
        let body = serde_json::json!({
            "to": pubkey_b,
            "payload": {
                "op": "pipeline",
                "steps": [
                    { "type": "generate", "seed": 42, "count": 10000 },
                    { "type": "filter", "predicate": { "field": "value", "op": "gt", "threshold": 0.5 } },
                    { "type": "aggregate", "function": "sum", "group_by": ["category"] },
                    { "type": "sort", "by": "count", "order": "desc" },
                    { "type": "limit", "n": 100 }
                ],
                "resources": { "max_memory_bytes": 268435456, "max_cpu_cores": 4 }
            }
        }).to_string();

        let resp = api_post(NS_A, "/compute/submit", &body)?;
        let task_id = resp["task_id"]
            .as_str()
            .context("missing task_id")?
            .to_string();
        println!("Complex task: {}...", &task_id[..16]);

        thread::sleep(Duration::from_secs(4));

        // task_id is blake3(sender || timestamp || payload). Match on B proves integrity.
        let tasks_b = api_get(NS_B, &format!("/compute/tasks/{}", pubkey_a))?;
        let b_list = tasks_b["tasks"].as_array().context("no tasks on B")?;
        let b_ids: Vec<&str> = b_list
            .iter()
            .filter_map(|t| t["task_id"].as_str())
            .collect();
        assert!(
            b_ids.contains(&task_id.as_str()),
            "complex task not received on B: {:?}",
            b_ids
        );
        println!("Complex task arrived intact on B");

        Ok(())
    })();

    node_a.kill().ok();
    node_b.kill().ok();
    cleanup_summitd();
    result.unwrap();
}

/// Compute: no tasks returns clean output.
#[test]
fn test_compute_no_tasks() {
    if !skip_unless_ready() {
        return;
    }

    let _lock = DAEMON_LOCK.lock().unwrap();
    cleanup_summitd();

    let env = [("SUMMIT_SERVICES__COMPUTE", "true")];
    let mut node_a = spawn_daemon(NS_A, VETH_A, &env);

    let result = (|| -> Result<()> {
        wait_for_api(NS_A, 40)?;

        let out = ctl(NS_A, &["compute", "tasks"])?;
        assert!(out.contains("No compute tasks"), "expected empty: {}", out);
        println!("compute tasks (empty): {}", out);

        let fake_pk = "0000000000000000000000000000000000000000000000000000000000000000";
        let out2 = ctl(NS_A, &["compute", "tasks", fake_pk])?;
        assert!(
            out2.contains("No compute tasks"),
            "expected empty for fake peer: {}",
            out2
        );

        Ok(())
    })();

    node_a.kill().ok();
    cleanup_summitd();
    result.unwrap();
}
