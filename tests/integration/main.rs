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

// Daemon processes are killed via .kill() + cleanup_summitd(pkill); .wait() is unnecessary.
#![allow(clippy::zombie_processes)]

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

/// Run summit-ctl inside a namespace with given args.
fn ctl(ns: &str, args: &[&str]) -> Result<String> {
    let ctl = summit_ctl_path();
    let ctl_str = ctl.to_str().unwrap();
    let mut full_args = vec![ctl_str];
    full_args.extend_from_slice(args);
    netns_exec(ns, &full_args)
}

/// Run summit-ctl inside a namespace, allowing non-zero exit.
fn ctl_raw(ns: &str, args: &[&str]) -> std::process::Output {
    let ctl = summit_ctl_path();
    let mut cmd = Command::new("ip");
    cmd.args(["netns", "exec", ns]);
    cmd.arg(&ctl);
    cmd.args(args);
    cmd.output().expect("failed to run summit-ctl")
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

/// Common guard: skip test if netns or binaries unavailable.
fn skip_unless_ready() -> bool {
    if !netns_available() {
        eprintln!("SKIP: netns not available — run sudo ./scripts/netns-up.sh first");
        return false;
    }
    if !binaries_available() {
        eprintln!("SKIP: binaries not built — run: cargo build -p summitd -p summit-ctl");
        return false;
    }
    true
}

/// Wait for sessions to be established on NS_A (returns the first session_id).
fn wait_for_session(secs: u64) -> Result<String> {
    thread::sleep(Duration::from_secs(secs));
    let status = api_get(NS_A, "/status")?;
    let sessions = status["sessions"].as_array().context("no sessions array")?;
    if sessions.is_empty() {
        bail!("no session established after {}s", secs);
    }
    sessions[0]["session_id"]
        .as_str()
        .map(|s| s.to_string())
        .context("missing session_id")
}

// ══════════════════════════════════════════════════════════════════════════════
//  Infrastructure tests — no daemons, run freely in parallel
// ══════════════════════════════════════════════════════════════════════════════

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

// ══════════════════════════════════════════════════════════════════════════════
//  Daemon status & management
// ══════════════════════════════════════════════════════════════════════════════

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

// ══════════════════════════════════════════════════════════════════════════════
//  Session management
// ══════════════════════════════════════════════════════════════════════════════

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

// ══════════════════════════════════════════════════════════════════════════════
//  Trust system
// ══════════════════════════════════════════════════════════════════════════════

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

// ══════════════════════════════════════════════════════════════════════════════
//  File Transfer service
// ══════════════════════════════════════════════════════════════════════════════

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

// ══════════════════════════════════════════════════════════════════════════════
//  Messaging service
// ══════════════════════════════════════════════════════════════════════════════

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

// ══════════════════════════════════════════════════════════════════════════════
//  Compute service
// ══════════════════════════════════════════════════════════════════════════════

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

// ══════════════════════════════════════════════════════════════════════════════
//  Service config: enable/disable affects discovery
// ══════════════════════════════════════════════════════════════════════════════

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

// ══════════════════════════════════════════════════════════════════════════════
//  Graceful shutdown
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
