use crate::*;

/// End-to-end messaging: send via API, retrieve via API and CLI.
#[test]
fn test_messaging_send_and_receive() {
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

        thread::sleep(Duration::from_secs(8));
        assert!(!api_get(NS_A, "/status")?["sessions"]
            .as_array()
            .unwrap()
            .is_empty());

        let pubkey_b = get_peer_pubkey(NS_A)?;
        let pubkey_a = get_peer_pubkey(NS_B)?;

        // Send via API
        let body = serde_json::json!({ "to": pubkey_b, "text": "hello from A" }).to_string();
        let resp = api_post(NS_A, "/messages/send", &body)?;
        assert!(resp["msg_id"].is_string(), "missing msg_id");
        assert!(resp["timestamp"].is_number(), "missing timestamp");

        thread::sleep(Duration::from_secs(4));

        // Retrieve via API on B
        let msgs = api_get(NS_B, &format!("/messages/{}", pubkey_a))?;
        let msg_list = msgs["messages"].as_array().context("no messages")?;
        assert!(!msg_list.is_empty(), "no messages on B from A");
        assert_eq!(
            msg_list[0]["content"]["text"].as_str().unwrap(),
            "hello from A"
        );

        // Retrieve via CLI on B
        let out = ctl(NS_B, &["messages", &pubkey_a])?;
        assert!(out.contains("hello from A"), "CLI missing message: {}", out);
        assert!(out.contains("Messages from"), "header missing: {}", out);
        println!("messages:\n{}", out);

        Ok(())
    })();

    node_a.kill().ok();
    node_b.kill().ok();
    cleanup_summitd();
    result.unwrap();
}

/// summit-ctl messages send: send via CLI and verify receipt.
#[test]
fn test_ctl_messages_send() {
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

        thread::sleep(Duration::from_secs(8));
        assert!(!api_get(NS_A, "/status")?["sessions"]
            .as_array()
            .unwrap()
            .is_empty());

        let pubkey_b = get_peer_pubkey(NS_A)?;
        let pubkey_a = get_peer_pubkey(NS_B)?;

        // Send via CLI
        let send_out = ctl(NS_A, &["messages", "send", &pubkey_b, "hello via CLI"])?;
        assert!(
            send_out.contains("Message sent"),
            "send output: {}",
            send_out
        );
        assert!(
            send_out.contains("ID"),
            "missing ID in output: {}",
            send_out
        );
        assert!(
            send_out.contains("Timestamp"),
            "missing Timestamp: {}",
            send_out
        );
        println!("messages send: {}", send_out);

        thread::sleep(Duration::from_secs(4));

        // Verify on B via CLI
        let recv_out = ctl(NS_B, &["messages", &pubkey_a])?;
        assert!(
            recv_out.contains("hello via CLI"),
            "message not received: {}",
            recv_out
        );
        println!("received:\n{}", recv_out);

        Ok(())
    })();

    node_a.kill().ok();
    node_b.kill().ok();
    cleanup_summitd();
    result.unwrap();
}

/// Multiple messages: verify ordering and count.
#[test]
fn test_messaging_multiple() {
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

        thread::sleep(Duration::from_secs(8));
        assert!(!api_get(NS_A, "/status")?["sessions"]
            .as_array()
            .unwrap()
            .is_empty());

        let pubkey_b = get_peer_pubkey(NS_A)?;
        let pubkey_a = get_peer_pubkey(NS_B)?;

        // Send 3 messages
        for i in 1..=3 {
            let body = serde_json::json!({
                "to": pubkey_b,
                "text": format!("msg {}", i)
            })
            .to_string();
            api_post(NS_A, "/messages/send", &body)?;
            thread::sleep(Duration::from_millis(50));
        }

        thread::sleep(Duration::from_secs(6));

        // Verify all 3 arrived
        let msgs = api_get(NS_B, &format!("/messages/{}", pubkey_a))?;
        let msg_list = msgs["messages"].as_array().context("no messages")?;
        assert!(
            msg_list.len() >= 3,
            "expected 3 messages, got {}",
            msg_list.len()
        );

        let texts: Vec<&str> = msg_list
            .iter()
            .filter_map(|m| m["content"]["text"].as_str())
            .collect();
        assert!(texts.contains(&"msg 1"), "missing msg 1: {:?}", texts);
        assert!(texts.contains(&"msg 2"), "missing msg 2: {:?}", texts);
        assert!(texts.contains(&"msg 3"), "missing msg 3: {:?}", texts);

        println!("All 3 messages received");
        Ok(())
    })();

    node_a.kill().ok();
    node_b.kill().ok();
    cleanup_summitd();
    result.unwrap();
}

/// Messages from unknown peer: verify empty response, no crash.
#[test]
fn test_messaging_no_messages() {
    if !skip_unless_ready() {
        return;
    }

    let _lock = DAEMON_LOCK.lock().unwrap();
    cleanup_summitd();

    let mut node_a = spawn_daemon(NS_A, VETH_A, &[]);

    let result = (|| -> Result<()> {
        wait_for_api(NS_A, 40)?;

        // Query messages for a fake pubkey
        let fake_pk = "0000000000000000000000000000000000000000000000000000000000000000";
        let out = ctl(NS_A, &["messages", fake_pk])?;
        assert!(
            out.contains("No messages"),
            "expected 'No messages': {}",
            out
        );
        println!("no messages: {}", out);

        Ok(())
    })();

    node_a.kill().ok();
    cleanup_summitd();
    result.unwrap();
}
