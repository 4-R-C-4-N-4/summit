//! Summit integration test harness.
//!
//! Tests in this file run against real network namespaces.
//! Requires root and the netns environment to be up:
//!
//!   sudo ./scripts/netns-up.sh
//!   sudo cargo test --test integration
//!
//! Daemon tests (those that spawn summitd) run serialized via DAEMON_LOCK
//! to avoid port 9001 conflicts between parallel tests.

use anyhow::{bail, Context, Result};
use serde_json::Value;
use std::path::PathBuf;
use std::process::{Child, Command};
use std::thread;
use std::time::Duration;

// ── Constants ─────────────────────────────────────────────────────────────────

pub const NS_A: &str = "summit-a";
pub const NS_B: &str = "summit-b";
pub const VETH_A: &str = "veth-a";
pub const VETH_B: &str = "veth-b";

/// Serializes all daemon-based tests so they don't conflict on port 9001.
static DAEMON_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

// ── Binary paths ──────────────────────────────────────────────────────────────

fn summitd_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .join("target/debug/summitd")
}

fn summit_ctl_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .join("target/debug/summit-ctl")
}

fn binaries_available() -> bool {
    summitd_path().exists() && summit_ctl_path().exists()
}

// ── Process helpers ───────────────────────────────────────────────────────────

/// Kill all running summitd processes to ensure clean test state.
fn cleanup_summitd() {
    Command::new("pkill").args(["-9", "summitd"]).output().ok();
    thread::sleep(Duration::from_millis(500));
}

/// Spawn a summitd in the given namespace on the given interface.
/// `extra_env` is a list of (key, value) pairs appended to the environment.
fn spawn_daemon(ns: &str, iface: &str, extra_env: &[(&str, &str)]) -> Child {
    let mut cmd = Command::new("ip");
    cmd.args(["netns", "exec", ns]);
    cmd.arg(summitd_path());
    cmd.arg(iface);
    cmd.env("RUST_LOG", "info");
    // Unique cache dir per daemon so they don't share on-disk state
    cmd.env(
        "SUMMIT_CACHE",
        format!("/tmp/summit-cache-{}-{}", ns, std::process::id()),
    );
    // Unique config path per daemon
    cmd.env(
        "SUMMIT_CONFIG",
        format!("/tmp/summit-config-{}-{}.toml", ns, std::process::id()),
    );
    for (k, v) in extra_env {
        cmd.env(k, v);
    }
    cmd.spawn().expect("failed to spawn summitd")
}

/// Wait until the REST API is reachable inside a namespace.
/// Polls up to `max_attempts * 500 ms`.
fn wait_for_api(ns: &str, max_attempts: u32) -> Result<()> {
    for attempt in 1..=max_attempts {
        let result = Command::new("ip")
            .args(["netns", "exec", ns])
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

        if let Ok(out) = result {
            if out.stdout == b"200" {
                return Ok(());
            }
        }

        if attempt < max_attempts {
            thread::sleep(Duration::from_millis(500));
        }
    }
    bail!("API not ready in {} after {} attempts", ns, max_attempts)
}

/// Run a JSON GET against the daemon API inside a namespace.
fn api_get(ns: &str, path: &str) -> Result<Value> {
    let url = format!("http://127.0.0.1:9001/api{}", path);
    let out = Command::new("ip")
        .args(["netns", "exec", ns])
        .args(["curl", "-sf", &url])
        .output()
        .with_context(|| format!("curl GET {} in {}", path, ns))?;

    if !out.status.success() {
        bail!(
            "curl GET {} in {} failed: {}",
            path,
            ns,
            String::from_utf8_lossy(&out.stderr)
        );
    }

    serde_json::from_slice(&out.stdout).context("failed to parse JSON")
}

/// Run a JSON POST against the daemon API inside a namespace.
fn api_post(ns: &str, path: &str, body: &str) -> Result<Value> {
    let url = format!("http://127.0.0.1:9001/api{}", path);
    let out = Command::new("ip")
        .args(["netns", "exec", ns])
        .args([
            "curl",
            "-sf",
            "-X",
            "POST",
            "-H",
            "Content-Type: application/json",
            "-d",
            body,
            &url,
        ])
        .output()
        .with_context(|| format!("curl POST {} in {}", path, ns))?;

    if !out.status.success() {
        bail!(
            "curl POST {} in {} failed: {}",
            path,
            ns,
            String::from_utf8_lossy(&out.stderr)
        );
    }

    serde_json::from_slice(&out.stdout).context("failed to parse JSON")
}

/// Return the first peer's `public_key` hex string visible from `ns`.
fn get_peer_pubkey(ns: &str) -> Result<String> {
    let resp = api_get(ns, "/peers")?;
    let peers = resp["peers"].as_array().context("no peers array")?;

    if peers.is_empty() {
        bail!("no peers discovered yet in {}", ns);
    }

    peers[0]["public_key"]
        .as_str()
        .map(|s| s.to_string())
        .context("no public_key field")
}

/// Trust a peer on `ns` via the REST API. Returns the number of flushed chunks.
fn trust_peer(ns: &str, peer_pubkey: &str) -> Result<usize> {
    let body = serde_json::json!({ "public_key": peer_pubkey }).to_string();
    let resp = api_post(ns, "/trust/add", &body)?;
    Ok(resp["flushed_chunks"].as_u64().unwrap_or(0) as usize)
}

// ── Namespace helpers ─────────────────────────────────────────────────────────

/// Run a command inside a network namespace.
/// Returns stdout as a String on success, error on non-zero exit.
pub fn netns_exec(ns: &str, args: &[&str]) -> Result<String> {
    let mut cmd = Command::new("ip");
    cmd.args(["netns", "exec", ns]);
    cmd.args(args);

    let output = cmd
        .output()
        .with_context(|| format!("failed to run: ip netns exec {ns} {args:?}"))?;

    if output.status.success() {
        Ok(String::from_utf8_lossy(&output.stdout).to_string())
    } else {
        bail!(
            "command failed in {ns}: {args:?}\nstderr: {}",
            String::from_utf8_lossy(&output.stderr)
        )
    }
}

/// Get the link-local IPv6 address of an interface inside a namespace.
/// Returns the address with interface scope, e.g. "fe80::1%veth-a".
pub fn link_local_addr(ns: &str, iface: &str) -> Result<String> {
    let output = netns_exec(ns, &["ip", "-6", "addr", "show", iface])?;

    for line in output.lines() {
        let line = line.trim();
        if line.starts_with("inet6 fe80::") {
            let addr = line
                .split_whitespace()
                .nth(1)
                .context("unexpected ip addr output format")?;
            let addr = addr.split('/').next().unwrap();
            return Ok(format!("{addr}%{iface}"));
        }
    }

    bail!("no link-local address found on {iface} in {ns}")
}

/// Check whether the netns environment is up.
pub fn netns_available() -> bool {
    Command::new("ip")
        .args(["netns", "exec", NS_A, "ip", "link", "show"])
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

// ── Infrastructure tests ───────────────────────────────────────────────────────
// These do not spawn daemons and run freely in parallel.

/// Verify the namespace environment is set up and interfaces are live.
#[test]
fn test_namespaces_exist() {
    if !netns_available() {
        eprintln!("SKIP: netns not available — run sudo ./scripts/netns-up.sh first");
        return;
    }

    let out_a =
        netns_exec(NS_A, &["ip", "link", "show", VETH_A]).expect("veth-a should exist in summit-a");
    assert!(out_a.contains(VETH_A), "veth-a not found in summit-a");

    let out_b =
        netns_exec(NS_B, &["ip", "link", "show", VETH_B]).expect("veth-b should exist in summit-b");
    assert!(out_b.contains(VETH_B), "veth-b not found in summit-b");

    println!("Both namespaces exist with correct interfaces.");
}

/// Verify link-local IPv6 addresses are assigned on both interfaces.
#[test]
fn test_link_local_addresses_assigned() {
    if !netns_available() {
        eprintln!("SKIP: netns not available");
        return;
    }

    let addr_a = link_local_addr(NS_A, VETH_A).expect("summit-a should have a link-local address");
    let addr_b = link_local_addr(NS_B, VETH_B).expect("summit-b should have a link-local address");

    println!("summit-a: {addr_a}");
    println!("summit-b: {addr_b}");

    assert!(
        addr_a.starts_with("fe80::"),
        "expected link-local in summit-a"
    );
    assert!(
        addr_b.starts_with("fe80::"),
        "expected link-local in summit-b"
    );
    assert_ne!(addr_a, addr_b, "addresses should be different");
}

/// Verify the two namespaces can reach each other via ping6.
#[test]
fn test_ping_a_to_b() {
    if !netns_available() {
        eprintln!("SKIP: netns not available");
        return;
    }

    let addr_b_raw =
        link_local_addr(NS_B, VETH_B).expect("summit-b should have a link-local address");
    let addr_b = addr_b_raw
        .split('%')
        .next()
        .map(|a| format!("{a}%{VETH_A}"))
        .unwrap();

    println!("Pinging {addr_b} from summit-a...");
    let result = netns_exec(NS_A, &["ping", "-6", "-c", "3", "-W", "2", &addr_b]);
    match &result {
        Ok(out) => println!("{out}"),
        Err(e) => panic!("ping6 from summit-a to summit-b failed: {e}"),
    }
    assert!(result.is_ok());
}

#[test]
fn test_ping_b_to_a() {
    if !netns_available() {
        eprintln!("SKIP: netns not available");
        return;
    }

    let addr_a_raw =
        link_local_addr(NS_A, VETH_A).expect("summit-a should have a link-local address");
    let addr_a = addr_a_raw
        .split('%')
        .next()
        .map(|a| format!("{a}%{VETH_B}"))
        .unwrap();

    println!("Pinging {addr_a} from summit-b...");
    let result = netns_exec(NS_B, &["ping", "-6", "-c", "3", "-W", "2", &addr_a]);
    match &result {
        Ok(out) => println!("{out}"),
        Err(e) => panic!("ping6 from summit-b to summit-a failed: {e}"),
    }
    assert!(result.is_ok());
}

// ── Daemon tests ───────────────────────────────────────────────────────────────
// Each acquires DAEMON_LOCK and calls cleanup_summitd() at start/end.

/// Verify that the daemon starts, its REST API is reachable, and /api/status
/// returns the expected JSON shape. Also smoke-tests `summit-ctl status`.
#[test]
fn test_daemon_status() {
    if !netns_available() {
        eprintln!("SKIP: netns not available");
        return;
    }
    if !binaries_available() {
        eprintln!("SKIP: binaries not built — run: cargo build -p summitd -p summit-ctl");
        return;
    }

    let _lock = DAEMON_LOCK.lock().unwrap();
    cleanup_summitd();

    let mut node_a = spawn_daemon(NS_A, VETH_A, &[]);
    let mut node_b = spawn_daemon(NS_B, VETH_B, &[]);

    let result = (|| -> Result<()> {
        wait_for_api(NS_A, 40).context("summit-a API not ready")?;
        wait_for_api(NS_B, 40).context("summit-b API not ready")?;

        let status_a = api_get(NS_A, "/status")?;
        assert!(
            status_a["peers_discovered"].is_number(),
            "status missing peers_discovered"
        );
        assert!(status_a["sessions"].is_array(), "status missing sessions");
        assert!(
            status_a["cache"]["chunks"].is_number(),
            "status missing cache.chunks"
        );

        let status_b = api_get(NS_B, "/status")?;
        assert!(status_b["peers_discovered"].is_number());

        // summit-ctl status end-to-end
        let ctl_out = netns_exec(NS_A, &[summit_ctl_path().to_str().unwrap(), "status"])?;
        assert!(
            ctl_out.contains("Summit Daemon Status"),
            "unexpected status output: {}",
            ctl_out
        );
        println!("Node A status:\n{}", ctl_out);

        Ok(())
    })();

    node_a.kill().ok();
    node_b.kill().ok();
    cleanup_summitd();

    result.unwrap();
}

/// Verify that both peers discover each other and the service announcements
/// carry the expected count (file_transfer + messaging = 2 by default) with
/// `is_complete = true` once all per-service datagrams have arrived.
#[test]
fn test_multi_service_peer_discovery() {
    if !netns_available() {
        eprintln!("SKIP: netns not available");
        return;
    }
    if !binaries_available() {
        eprintln!("SKIP: binaries not built");
        return;
    }

    let _lock = DAEMON_LOCK.lock().unwrap();
    cleanup_summitd();

    let mut node_a = spawn_daemon(NS_A, VETH_A, &[]);
    let mut node_b = spawn_daemon(NS_B, VETH_B, &[]);

    let result = (|| -> Result<()> {
        wait_for_api(NS_A, 40)?;
        wait_for_api(NS_B, 40)?;

        // Broadcast loop fires ~every 2 s; 6 s gives at least 3 rounds
        thread::sleep(Duration::from_secs(6));

        // Node A should see Node B with both services
        let peers_a = api_get(NS_A, "/peers")?;
        let peers_list_a = peers_a["peers"].as_array().context("no peers array on A")?;
        assert!(!peers_list_a.is_empty(), "node A has no peers after 6s");

        let peer_b = &peers_list_a[0];
        let svc_count = peer_b["service_count"].as_u64().unwrap_or(0);
        assert!(
            svc_count >= 2,
            "expected >=2 services (file_transfer + messaging), got {}",
            svc_count
        );
        assert!(
            peer_b["is_complete"].as_bool().unwrap_or(false),
            "peer B not complete on node A"
        );
        let services = peer_b["services"].as_array().context("no services array")?;
        assert!(
            services.len() >= 2,
            "expected >=2 service hashes, got {}",
            services.len()
        );
        println!(
            "Node A sees peer with {}/{} service(s), complete={}",
            services.len(),
            svc_count,
            peer_b["is_complete"]
        );

        // Node B should see Node A
        let peers_b = api_get(NS_B, "/peers")?;
        let peers_list_b = peers_b["peers"].as_array().context("no peers array on B")?;
        assert!(!peers_list_b.is_empty(), "node B has no peers after 6s");
        assert!(
            peers_list_b[0]["is_complete"].as_bool().unwrap_or(false),
            "peer A not complete on node B"
        );

        // summit-ctl peers should display the services line
        let ctl_peers = netns_exec(NS_A, &[summit_ctl_path().to_str().unwrap(), "peers"])?;
        assert!(
            ctl_peers.contains("services"),
            "peers output missing 'services': {}",
            ctl_peers
        );
        println!("summit-ctl peers:\n{}", ctl_peers);

        Ok(())
    })();

    node_a.kill().ok();
    node_b.kill().ok();
    cleanup_summitd();

    result.unwrap();
}

/// Verify that a Noise session is established between two nodes after peer
/// discovery (the lower-key node initiates automatically, no trust required).
/// Also smoke-tests `summit-ctl sessions inspect`.
#[test]
fn test_session_establishment() {
    if !netns_available() {
        eprintln!("SKIP: netns not available");
        return;
    }
    if !binaries_available() {
        eprintln!("SKIP: binaries not built");
        return;
    }

    let _lock = DAEMON_LOCK.lock().unwrap();
    cleanup_summitd();

    let mut node_a = spawn_daemon(NS_A, VETH_A, &[]);
    let mut node_b = spawn_daemon(NS_B, VETH_B, &[]);

    let result = (|| -> Result<()> {
        wait_for_api(NS_A, 40)?;
        wait_for_api(NS_B, 40)?;

        // Discovery ~2 s + initiator tick every 3 s -> allow 8 s total
        thread::sleep(Duration::from_secs(8));

        let status_a = api_get(NS_A, "/status")?;
        let sessions = status_a["sessions"]
            .as_array()
            .context("no sessions array")?;
        assert!(
            !sessions.is_empty(),
            "node A has no sessions after 8s — handshake failed"
        );

        let session = &sessions[0];
        assert!(session["session_id"].is_string(), "missing session_id");
        assert!(session["peer_pubkey"].is_string(), "missing peer_pubkey");
        assert!(session["contract"].is_string(), "missing contract");

        let session_id = session["session_id"].as_str().unwrap();
        println!(
            "Session established: id={}... contract={}",
            &session_id[..16],
            session["contract"]
        );

        // sessions inspect end-to-end
        let inspect = netns_exec(
            NS_A,
            &[
                summit_ctl_path().to_str().unwrap(),
                "sessions",
                "inspect",
                session_id,
            ],
        )?;
        assert!(
            inspect.contains("Session Details"),
            "unexpected inspect output: {}",
            inspect
        );
        println!("Session inspect:\n{}", inspect);

        Ok(())
    })();

    node_a.kill().ok();
    node_b.kill().ok();
    cleanup_summitd();

    result.unwrap();
}

/// Verify the three-tier trust model:
/// - Peers start as Untrusted
/// - Explicit `trust add` via the API changes the level to Trusted
/// - `trust block` sets Blocked
/// - `summit-ctl trust list` reflects the rules
#[test]
fn test_trust_system() {
    if !netns_available() {
        eprintln!("SKIP: netns not available");
        return;
    }
    if !binaries_available() {
        eprintln!("SKIP: binaries not built");
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

        // Before any explicit trust: rules list is empty
        let trust_before = api_get(NS_A, "/trust")?;
        assert!(
            trust_before["rules"].as_array().unwrap().is_empty(),
            "expected empty trust rules initially"
        );

        // Peer appears as Untrusted
        let peers_a = api_get(NS_A, "/peers")?;
        let peer = &peers_a["peers"].as_array().context("no peers on A")?[0];
        assert_eq!(
            peer["trust_level"].as_str().unwrap(),
            "Untrusted",
            "peer should start Untrusted"
        );

        let pubkey_b = get_peer_pubkey(NS_A)?;
        println!("B pubkey: {}...", &pubkey_b[..16]);

        // Trust B from A
        let flushed = trust_peer(NS_A, &pubkey_b)?;
        println!("Trusted B; flushed {} buffered chunk(s)", flushed);

        let trust_after = api_get(NS_A, "/trust")?;
        let rules = trust_after["rules"].as_array().unwrap();
        assert_eq!(rules.len(), 1, "expected 1 rule after trust");
        assert_eq!(rules[0]["level"].as_str().unwrap(), "Trusted");
        assert_eq!(rules[0]["public_key"].as_str().unwrap(), pubkey_b);

        // summit-ctl trust list
        let ctl_trust = netns_exec(
            NS_A,
            &[summit_ctl_path().to_str().unwrap(), "trust", "list"],
        )?;
        assert!(
            ctl_trust.contains("Trusted") || ctl_trust.contains(&pubkey_b[..16]),
            "unexpected trust list: {}",
            ctl_trust
        );
        println!("Trust list:\n{}", ctl_trust);

        // Block A from B
        let pubkey_a = get_peer_pubkey(NS_B)?;
        let block_body = serde_json::json!({ "public_key": pubkey_a }).to_string();
        api_post(NS_B, "/trust/block", &block_body)?;

        let trust_b = api_get(NS_B, "/trust")?;
        let rules_b = trust_b["rules"].as_array().unwrap();
        assert_eq!(rules_b.len(), 1, "expected 1 block rule on B");
        assert_eq!(rules_b[0]["level"].as_str().unwrap(), "Blocked");

        println!("Verified trust system");
        Ok(())
    })();

    node_a.kill().ok();
    node_b.kill().ok();
    cleanup_summitd();

    result.unwrap();
}

/// Verify that `SUMMIT_TRUST__AUTO_TRUST=true` causes all discovered peers to
/// appear as Trusted without any explicit trust call.
#[test]
fn test_auto_trust_config() {
    if !netns_available() {
        eprintln!("SKIP: netns not available");
        return;
    }
    if !binaries_available() {
        eprintln!("SKIP: binaries not built");
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

        let peers_a = api_get(NS_A, "/peers")?;
        let peers_list = peers_a["peers"].as_array().context("no peers")?;
        assert!(!peers_list.is_empty(), "no peers after 6s");

        let trust_level = peers_list[0]["trust_level"].as_str().unwrap_or("?");
        assert_eq!(
            trust_level, "Trusted",
            "expected Trusted with auto_trust=true, got {}",
            trust_level
        );
        println!(
            "Auto-trust: peer {}... is {}",
            &peers_list[0]["public_key"].as_str().unwrap()[..16],
            trust_level
        );
        println!("Verified auto-trust config");
        Ok(())
    })();

    node_a.kill().ok();
    node_b.kill().ok();
    cleanup_summitd();

    result.unwrap();
}

/// End-to-end file transfer: A sends a file, B receives and reassembles it.
/// Uses auto-trust so no explicit trust call is needed.
#[test]
fn test_file_transfer_two_nodes() {
    if !netns_available() {
        eprintln!("SKIP: netns not available");
        return;
    }
    if !binaries_available() {
        eprintln!("SKIP: binaries not built");
        return;
    }

    let _lock = DAEMON_LOCK.lock().unwrap();
    cleanup_summitd();
    std::fs::remove_dir_all("/tmp/summit-received").ok();

    let auto_env = [("SUMMIT_TRUST__AUTO_TRUST", "true")];
    let mut node_a = spawn_daemon(NS_A, VETH_A, &auto_env);
    let mut node_b = spawn_daemon(NS_B, VETH_B, &auto_env);

    let test_content = "Summit integration test — multi-service file transfer";
    let test_file = "/tmp/summit-integration-transfer.txt";
    std::fs::write(test_file, test_content).unwrap();

    let result = (|| -> Result<()> {
        wait_for_api(NS_A, 40)?;
        wait_for_api(NS_B, 40)?;

        println!("Waiting for peer discovery and session establishment...");
        thread::sleep(Duration::from_secs(8));

        // Verify session exists before sending
        let status_a = api_get(NS_A, "/status")?;
        assert!(
            !status_a["sessions"].as_array().unwrap().is_empty(),
            "no session on A before send"
        );

        // Send file from A (broadcast to all trusted peers)
        let send_out = netns_exec(
            NS_A,
            &[summit_ctl_path().to_str().unwrap(), "send", test_file],
        )?;
        assert!(
            send_out.contains("File queued"),
            "unexpected send output: {}",
            send_out
        );
        println!("Send output: {}", send_out);

        println!("Waiting for transfer to complete...");
        thread::sleep(Duration::from_secs(8));

        // Check B received it
        let files_out = netns_exec(NS_B, &[summit_ctl_path().to_str().unwrap(), "files"])?;
        println!("Node B files:\n{}", files_out);
        assert!(
            files_out.contains("summit-integration-transfer.txt"),
            "file not received on B: {}",
            files_out
        );

        // Verify content
        let received =
            std::fs::read_to_string("/tmp/summit-received/summit-integration-transfer.txt")
                .context("received file not found")?;
        assert_eq!(received, test_content, "file content mismatch");

        println!("Verified file transfer end-to-end");
        Ok(())
    })();

    node_a.kill().ok();
    node_b.kill().ok();
    cleanup_summitd();
    std::fs::remove_file(test_file).ok();

    result.unwrap();
}

/// Verify the messaging service: A sends a text message to B via the REST API,
/// and B can retrieve it from `/api/messages/<pubkey>`.
/// Also tests `summit-ctl messages` end-to-end.
#[test]
fn test_messaging_service() {
    if !netns_available() {
        eprintln!("SKIP: netns not available");
        return;
    }
    if !binaries_available() {
        eprintln!("SKIP: binaries not built");
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

        assert!(
            !api_get(NS_A, "/status")?["sessions"]
                .as_array()
                .unwrap()
                .is_empty(),
            "no session before messaging test"
        );

        let pubkey_b = get_peer_pubkey(NS_A)?;
        println!("Sending message to B ({}...)", &pubkey_b[..16]);

        // Send text message A -> B
        let body = serde_json::json!({ "to": pubkey_b, "text": "hello from integration test" })
            .to_string();
        let send_resp = api_post(NS_A, "/messages/send", &body)?;
        assert!(
            send_resp["msg_id"].is_string(),
            "send_resp missing msg_id: {:?}",
            send_resp
        );
        println!(
            "Message queued, id={}...",
            &send_resp["msg_id"].as_str().unwrap()[..16]
        );

        // Wait for chunk delivery
        thread::sleep(Duration::from_secs(4));

        // Get A's pubkey as seen from B, then check B's message store
        let pubkey_a = get_peer_pubkey(NS_B)?;
        let msgs_resp = api_get(NS_B, &format!("/messages/{}", pubkey_a))?;
        let msgs = msgs_resp["messages"]
            .as_array()
            .context("no messages array")?;
        assert!(!msgs.is_empty(), "no messages on B from A after 4s");

        let text = msgs[0]["content"]["text"].as_str().unwrap_or("");
        assert_eq!(
            text, "hello from integration test",
            "message text mismatch: {:?}",
            msgs[0]
        );

        // summit-ctl messages end-to-end
        let ctl_msgs = netns_exec(
            NS_B,
            &[summit_ctl_path().to_str().unwrap(), "messages", &pubkey_a],
        )?;
        assert!(
            ctl_msgs.contains("hello from integration test"),
            "summit-ctl messages missing text: {}",
            ctl_msgs
        );
        println!("summit-ctl messages:\n{}", ctl_msgs);

        println!("Verified messaging service end-to-end");
        Ok(())
    })();

    node_a.kill().ok();
    node_b.kill().ok();
    cleanup_summitd();

    result.unwrap();
}

/// Verify that disabling a service via env var reduces the announced
/// `service_count` to 1. Node A disables messaging; node B (default) should
/// see `service_count = 1` and `is_complete = true` for A.
#[test]
fn test_service_config_disable_messaging() {
    if !netns_available() {
        eprintln!("SKIP: netns not available");
        return;
    }
    if !binaries_available() {
        eprintln!("SKIP: binaries not built");
        return;
    }

    let _lock = DAEMON_LOCK.lock().unwrap();
    cleanup_summitd();

    // A: only file_transfer; B: default (both services)
    let a_env = [("SUMMIT_SERVICES__MESSAGING", "false")];
    let mut node_a = spawn_daemon(NS_A, VETH_A, &a_env);
    let mut node_b = spawn_daemon(NS_B, VETH_B, &[]);

    let result = (|| -> Result<()> {
        wait_for_api(NS_A, 40)?;
        wait_for_api(NS_B, 40)?;

        thread::sleep(Duration::from_secs(6));

        // B looks at A — should see exactly 1 service
        let peers_b = api_get(NS_B, "/peers")?;
        let peers_list = peers_b["peers"].as_array().context("no peers on B")?;
        assert!(!peers_list.is_empty(), "B has no peers after 6s");

        let peer_a_on_b = &peers_list[0];
        let svc_count = peer_a_on_b["service_count"].as_u64().unwrap_or(99);
        assert_eq!(
            svc_count, 1,
            "expected 1 service from A (messaging disabled), got {}",
            svc_count
        );
        assert!(
            peer_a_on_b["is_complete"].as_bool().unwrap_or(false),
            "peer A not marked complete on B"
        );
        println!(
            "A announced {} service(s), complete={} — messaging disabled",
            svc_count, peer_a_on_b["is_complete"]
        );

        // A itself should see B with 2 services (B has default config)
        let peers_a = api_get(NS_A, "/peers")?;
        let peers_list_a = peers_a["peers"].as_array().context("no peers on A")?;
        assert!(!peers_list_a.is_empty(), "A has no peers after 6s");
        let svc_count_b = peers_list_a[0]["service_count"].as_u64().unwrap_or(0);
        assert!(
            svc_count_b >= 2,
            "expected >=2 services from B (default config), got {}",
            svc_count_b
        );

        println!("Verified service config disable");
        Ok(())
    })();

    node_a.kill().ok();
    node_b.kill().ok();
    cleanup_summitd();

    result.unwrap();
}

/// End-to-end compute task submission: A submits a simple echo task to B via the
/// REST API. The task is stored locally on A immediately, then delivered as a
/// bulk chunk over the encrypted session. B receives it and stores it in its
/// ComputeStore. Both sides are verified, plus the `summit-ctl compute` commands
/// are smoke-tested end-to-end.
#[test]
fn test_compute_task_simple() {
    if !netns_available() {
        eprintln!("SKIP: netns not available");
        return;
    }
    if !binaries_available() {
        eprintln!("SKIP: binaries not built");
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

        println!("Waiting for peer discovery and session establishment...");
        thread::sleep(Duration::from_secs(8));

        assert!(
            !api_get(NS_A, "/status")?["sessions"]
                .as_array()
                .unwrap()
                .is_empty(),
            "no session on A before compute test"
        );

        let pubkey_b = get_peer_pubkey(NS_A)?;
        let pubkey_a = get_peer_pubkey(NS_B)?;
        println!("B pubkey: {}...", &pubkey_b[..16]);
        println!("A pubkey: {}...", &pubkey_a[..16]);

        // Submit a simple echo task: A -> B
        let body = serde_json::json!({
            "to": pubkey_b,
            "payload": {
                "op": "echo",
                "input": "hello from node A"
            }
        })
        .to_string();

        let submit_resp = api_post(NS_A, "/compute/submit", &body)?;
        let task_id = submit_resp["task_id"]
            .as_str()
            .context("submit response missing task_id")?
            .to_string();
        assert!(
            submit_resp["timestamp"].is_number(),
            "submit response missing timestamp"
        );
        println!("Task submitted: {}...", &task_id[..16]);

        // A stores the task locally at submit time — no delivery wait needed
        let tasks_on_a = api_get(NS_A, &format!("/compute/tasks/{}", pubkey_b))?;
        let a_tasks = tasks_on_a["tasks"]
            .as_array()
            .context("no tasks array on A")?;
        assert!(
            !a_tasks.is_empty(),
            "task not in A's local store after submit"
        );
        assert_eq!(
            a_tasks[0]["task_id"].as_str().unwrap(),
            task_id,
            "task_id mismatch in A's local store"
        );
        assert_eq!(
            a_tasks[0]["status"].as_str().unwrap(),
            "Queued",
            "unexpected initial status on A"
        );
        println!("Task confirmed in A's local store");

        println!("Waiting for chunk delivery to B...");
        thread::sleep(Duration::from_secs(4));

        // B received the chunk and stored the task keyed by A's pubkey
        let tasks_on_b = api_get(NS_B, &format!("/compute/tasks/{}", pubkey_a))?;
        let b_tasks = tasks_on_b["tasks"]
            .as_array()
            .context("no tasks array on B")?;
        assert!(!b_tasks.is_empty(), "echo task not received on B after 4s");
        assert_eq!(
            b_tasks[0]["task_id"].as_str().unwrap(),
            task_id,
            "task_id mismatch on B — payload may have been corrupted in transit"
        );
        assert_eq!(
            b_tasks[0]["status"].as_str().unwrap(),
            "Queued",
            "expected Queued on B, got {}",
            b_tasks[0]["status"]
        );
        println!(
            "Echo task {}... arrived on B with status={}",
            &task_id[..16],
            b_tasks[0]["status"]
        );

        // summit-ctl compute tasks: check B's task list shows the task from A
        let ctl_tasks = netns_exec(
            NS_B,
            &[
                summit_ctl_path().to_str().unwrap(),
                "compute",
                "tasks",
                &pubkey_a,
            ],
        )?;
        assert!(
            ctl_tasks.contains(&task_id[..16]),
            "summit-ctl compute tasks missing task_id: {}",
            ctl_tasks
        );
        println!("summit-ctl compute tasks:\n{}", ctl_tasks);

        // summit-ctl compute submit: B submits a task back to A via the CLI
        let ctl_submit = netns_exec(
            NS_B,
            &[
                summit_ctl_path().to_str().unwrap(),
                "compute",
                "submit",
                &pubkey_a,
                r#"{"op":"echo","input":"hello from node B"}"#,
            ],
        )?;
        assert!(
            ctl_submit.contains("Task ID"),
            "unexpected compute submit output: {}",
            ctl_submit
        );
        println!("summit-ctl compute submit:\n{}", ctl_submit);

        // summit-ctl services: compute should show as enabled on both nodes
        let ctl_services_a = netns_exec(NS_A, &[summit_ctl_path().to_str().unwrap(), "services"])?;
        assert!(
            ctl_services_a.contains("compute") && ctl_services_a.contains("enabled"),
            "compute not shown as enabled on A: {}",
            ctl_services_a
        );
        println!("summit-ctl services (A):\n{}", ctl_services_a);

        println!("Verified simple compute task end-to-end");
        Ok(())
    })();

    node_a.kill().ok();
    node_b.kill().ok();
    cleanup_summitd();

    result.unwrap();
}

/// End-to-end compute task with a complex multi-step data pipeline payload.
///
/// Verifies that a deeply nested JSON task definition survives the full
/// serialise → chunk → encrypt → transmit → decrypt → deserialise round-trip
/// intact. The task_id is a BLAKE3 hash of the sender, timestamp, and the
/// complete payload, so an id match on B proves byte-level payload integrity.
///
/// Also submits a second task (matrix multiply) to verify the store handles
/// multiple concurrent tasks from the same peer correctly.
#[test]
fn test_compute_task_challenging() {
    if !netns_available() {
        eprintln!("SKIP: netns not available");
        return;
    }
    if !binaries_available() {
        eprintln!("SKIP: binaries not built");
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

        println!("Waiting for peer discovery and session establishment...");
        thread::sleep(Duration::from_secs(8));

        assert!(
            !api_get(NS_A, "/status")?["sessions"]
                .as_array()
                .unwrap()
                .is_empty(),
            "no session before challenging compute test"
        );

        let pubkey_b = get_peer_pubkey(NS_A)?;
        let pubkey_a = get_peer_pubkey(NS_B)?;

        // Task 1: a five-stage data pipeline.
        // Stages: generate synthetic data → filter on a predicate → aggregate
        // by category → sort descending → take top-N. This represents a
        // realistic distributed analytics workload.
        let pipeline_body = serde_json::json!({
            "to": pubkey_b,
            "payload": {
                "op": "pipeline",
                "steps": [
                    {
                        "type": "generate",
                        "seed": 42,
                        "count": 10000,
                        "distribution": "uniform",
                        "range": [0.0, 1.0]
                    },
                    {
                        "type": "filter",
                        "predicate": {
                            "field": "value",
                            "op": "gt",
                            "threshold": 0.5
                        }
                    },
                    {
                        "type": "aggregate",
                        "function": "sum",
                        "group_by": ["category"],
                        "output_fields": ["category", "count", "total"]
                    },
                    {
                        "type": "sort",
                        "by": "count",
                        "order": "desc"
                    },
                    {
                        "type": "limit",
                        "n": 100
                    }
                ],
                "output_format": "jsonl",
                "timeout_ms": 30000,
                "priority": "high",
                "resources": {
                    "max_memory_bytes": 268435456,
                    "max_cpu_cores": 4,
                    "max_wall_time_ms": 30000
                }
            }
        })
        .to_string();

        let pipeline_resp = api_post(NS_A, "/compute/submit", &pipeline_body)?;
        let pipeline_id = pipeline_resp["task_id"]
            .as_str()
            .context("pipeline submit missing task_id")?
            .to_string();
        println!("Pipeline task submitted: {}...", &pipeline_id[..16]);

        // Small delay between submits so timestamps don't collide
        thread::sleep(Duration::from_millis(10));

        // Task 2: blocked matrix multiply over 64-bit integers, tiled for
        // cache efficiency. This represents a compute-intensive linear algebra
        // workload that would require coordination across CPU cores.
        let matmul_body = serde_json::json!({
            "to": pubkey_b,
            "payload": {
                "op": "matmul",
                "dtype": "int64",
                "tile_size": 64,
                "a": {
                    "rows": 512,
                    "cols": 512,
                    "data_ref": "chunk:a1b2c3d4e5f67890"
                },
                "b": {
                    "rows": 512,
                    "cols": 512,
                    "data_ref": "chunk:0987654321fedcba"
                },
                "accumulator": "kahan",
                "parallel": true,
                "resources": {
                    "max_memory_bytes": 536870912,
                    "max_cpu_cores": 8,
                    "max_wall_time_ms": 60000
                }
            }
        })
        .to_string();

        let matmul_resp = api_post(NS_A, "/compute/submit", &matmul_body)?;
        let matmul_id = matmul_resp["task_id"]
            .as_str()
            .context("matmul submit missing task_id")?
            .to_string();
        println!("Matrix multiply task submitted: {}...", &matmul_id[..16]);

        // Both task_ids must be distinct (payloads differ, so hashes differ)
        assert_ne!(
            pipeline_id, matmul_id,
            "distinct payloads must produce distinct task_ids"
        );

        // A's local store should already hold both tasks
        let tasks_on_a = api_get(NS_A, &format!("/compute/tasks/{}", pubkey_b))?;
        let a_tasks = tasks_on_a["tasks"].as_array().context("no tasks on A")?;
        assert!(
            a_tasks.len() >= 2,
            "expected 2 tasks in A's local store, got {}",
            a_tasks.len()
        );
        let a_ids: Vec<&str> = a_tasks
            .iter()
            .filter_map(|t| t["task_id"].as_str())
            .collect();
        assert!(
            a_ids.contains(&pipeline_id.as_str()),
            "pipeline task_id missing from A's store: {:?}",
            a_ids
        );
        assert!(
            a_ids.contains(&matmul_id.as_str()),
            "matmul task_id missing from A's store: {:?}",
            a_ids
        );
        println!("Both tasks confirmed in A's local store");

        println!("Waiting for chunk delivery to B...");
        thread::sleep(Duration::from_secs(4));

        // B should have received and stored both tasks from A
        let tasks_on_b = api_get(NS_B, &format!("/compute/tasks/{}", pubkey_a))?;
        let b_tasks = tasks_on_b["tasks"]
            .as_array()
            .context("no tasks array on B")?;
        assert!(
            b_tasks.len() >= 2,
            "expected 2 tasks on B from A, got {}",
            b_tasks.len()
        );

        let b_ids: Vec<&str> = b_tasks
            .iter()
            .filter_map(|t| t["task_id"].as_str())
            .collect();

        // task_id is blake3(sender || timestamp || payload_bytes).
        // A match on B proves the payload survived the round-trip byte-for-byte.
        assert!(
            b_ids.contains(&pipeline_id.as_str()),
            "pipeline task_id not received on B — payload corrupted in transit: {:?}",
            b_ids
        );
        assert!(
            b_ids.contains(&matmul_id.as_str()),
            "matmul task_id not received on B — payload corrupted in transit: {:?}",
            b_ids
        );

        for task in b_tasks {
            assert_eq!(
                task["status"].as_str().unwrap_or(""),
                "Queued",
                "unexpected status for {} on B",
                task["task_id"]
            );
        }
        println!(
            "Both tasks arrived on B: pipeline={}... matmul={}...",
            &pipeline_id[..16],
            &matmul_id[..16]
        );

        // Verify B's /api/services reflects compute as enabled
        let services = api_get(NS_B, "/services")?;
        let svc_list = services["services"]
            .as_array()
            .context("no services list")?;
        let compute_entry = svc_list
            .iter()
            .find(|s| s["name"].as_str() == Some("compute"))
            .context("compute not in services list")?;
        assert!(
            compute_entry["enabled"].as_bool().unwrap_or(false),
            "compute service not enabled on B"
        );
        assert_eq!(
            compute_entry["contract"].as_str().unwrap_or(""),
            "Bulk",
            "compute contract should be Bulk"
        );
        println!("Compute service confirmed enabled with Bulk contract on B");

        // summit-ctl compute tasks: B should list both tasks from A
        let ctl_tasks = netns_exec(
            NS_B,
            &[
                summit_ctl_path().to_str().unwrap(),
                "compute",
                "tasks",
                &pubkey_a,
            ],
        )?;
        assert!(
            ctl_tasks.contains(&pipeline_id[..16]),
            "ctl missing pipeline task: {}",
            ctl_tasks
        );
        assert!(
            ctl_tasks.contains(&matmul_id[..16]),
            "ctl missing matmul task: {}",
            ctl_tasks
        );
        println!("summit-ctl compute tasks (B):\n{}", ctl_tasks);

        println!("Verified challenging compute tasks end-to-end");
        Ok(())
    })();

    node_a.kill().ok();
    node_b.kill().ok();
    cleanup_summitd();

    result.unwrap();
}

/// Verify the /api/schema endpoint returns known schemas and `summit-ctl schema list` works.
#[test]
fn test_schema_list() {
    if !netns_available() {
        eprintln!("SKIP: netns not available");
        return;
    }
    if !binaries_available() {
        eprintln!("SKIP: binaries not built");
        return;
    }

    let _lock = DAEMON_LOCK.lock().unwrap();
    cleanup_summitd();

    let mut node_a = spawn_daemon(NS_A, VETH_A, &[]);

    let result = (|| -> Result<()> {
        wait_for_api(NS_A, 40)?;

        let resp = api_get(NS_A, "/schema")?;
        let schemas = resp["schemas"].as_array().context("no schemas array")?;
        assert!(!schemas.is_empty(), "schema list is empty");

        let names: Vec<&str> = schemas.iter().filter_map(|s| s["name"].as_str()).collect();
        println!("Known schemas: {:?}", names);
        assert!(
            names.iter().any(|n| n.contains("File")),
            "expected a File schema, got {:?}",
            names
        );

        let ctl_schema = netns_exec(
            NS_A,
            &[summit_ctl_path().to_str().unwrap(), "schema", "list"],
        )?;
        assert!(
            ctl_schema.contains("Known Schemas"),
            "unexpected schema output: {}",
            ctl_schema
        );
        println!("Schema list:\n{}", ctl_schema);

        Ok(())
    })();

    node_a.kill().ok();
    cleanup_summitd();

    result.unwrap();
}
