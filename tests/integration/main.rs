//! Summit integration test harness.
//!
//! Tests in this file run against real network namespaces.
//! Requires root and the netns environment to be up:
//!
//!   sudo ./scripts/netns-up.sh
//!   sudo cargo test --test integration
//!
//! Each test is responsible for any processes it spawns.
//! The namespace environment is shared — tests must not
//! interfere with each other's interfaces.

use anyhow::{bail, Context, Result};
use serde_json::Value;
use std::process::Command;
use std::thread;
use std::time::Duration;

// ── Harness ───────────────────────────────────────────────────────────────────

/// The two namespace names used throughout tests.
pub const NS_A: &str = "summit-a";
pub const NS_B: &str = "summit-b";
pub const VETH_A: &str = "veth-a";
pub const VETH_B: &str = "veth-b";

/// Kill all running summitd processes to ensure clean test state
fn cleanup_summitd() {
    // Kill any existing summitd processes
    Command::new("pkill").args(&["-9", "summitd"]).output().ok();

    // Small delay to ensure processes are gone
    thread::sleep(Duration::from_millis(500));
}

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
/// Returns the address without prefix length, e.g. "fe80::1%veth-a"
pub fn link_local_addr(ns: &str, iface: &str) -> Result<String> {
    let output = netns_exec(ns, &["ip", "-6", "addr", "show", iface])?;

    for line in output.lines() {
        let line = line.trim();
        if line.starts_with("inet6 fe80::") {
            // line looks like: "inet6 fe80::1/64 scope link"
            let addr = line
                .split_whitespace()
                .nth(1)
                .context("unexpected ip addr output format")?;
            // strip the /64 prefix length
            let addr = addr.split('/').next().unwrap();
            // append the interface scope
            return Ok(format!("{addr}%{iface}"));
        }
    }

    bail!("no link-local address found on {iface} in {ns}")
}

/// Check whether the netns environment is up.
/// Tests call this and skip gracefully if not running as root
/// or if namespaces haven't been created.
pub fn netns_available() -> bool {
    Command::new("ip")
        .args(["netns", "exec", NS_A, "ip", "link", "show"])
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

// ── Tests ─────────────────────────────────────────────────────────────────────

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
        "expected link-local address in summit-a"
    );
    assert!(
        addr_b.starts_with("fe80::"),
        "expected link-local address in summit-b"
    );
    assert_ne!(addr_a, addr_b, "addresses should be different");
}

/// Verify the two namespaces can reach each other via ping6.
/// This is the fundamental connectivity check for all future tests.
#[test]
fn test_ping_a_to_b() {
    if !netns_available() {
        eprintln!("SKIP: netns not available");
        return;
    }

    // Get B's address, but scope it to A's interface name
    let addr_b_raw =
        link_local_addr(NS_B, VETH_B).expect("summit-b should have a link-local address");
    // strip the %veth-b scope and replace with %veth-a (A's local interface)
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

    // Get A's address, but scope it to B's interface name
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

// ── File Transfer Tests ───────────────────────────────────────────────────────

// fn summitd_path() -> std::path::PathBuf {
//     std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
//     .parent().unwrap()
//     .parent().unwrap()
//     .join("target/debug/summitd")
// }
//
// fn summit_ctl_path() -> std::path::PathBuf {
//     std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
//     .parent().unwrap()
//     .parent().unwrap()
//     .join("target/debug/summit-ctl")
// }
//
// fn wait_for_api(netns: &str, max_attempts: u32) -> Result<(), String> {
//     for attempt in 1..=max_attempts {
//         let result = Command::new("ip")
//         .args(&["netns", "exec", netns])
//         .args(&["curl", "-s", "-o", "/dev/null", "-w", "%{http_code}", "http://127.0.0.1:9001/api/status"])
//         .output();
//
//         if let Ok(output) = result {
//             let status_code = String::from_utf8_lossy(&output.stdout);
//             if status_code == "200" {
//                 return Ok(());
//             }
//         }
//
//         if attempt < max_attempts {
//             thread::sleep(Duration::from_millis(500));
//         }
//     }
//     Err(format!("API not ready after {} attempts", max_attempts))
// }
//
// fn get_peer_pubkey(netns: &str) -> Result<String, String> {
//     let result = Command::new("ip")
//     .args(&["netns", "exec", netns])
//     .args(&["curl", "-s", "http://127.0.0.1:9001/api/peers"])
//     .output()
//     .map_err(|e| format!("curl failed: {}", e))?;
//
//     if !result.status.success() {
//         return Err(format!("curl failed with status: {}", result.status));
//     }
//
//     let json_str = String::from_utf8_lossy(&result.stdout);
//     let json: Value = serde_json::from_str(&json_str)
//     .map_err(|e| format!("failed to parse JSON: {}", e))?;
//
//     let peers = json["peers"].as_array()
//     .ok_or("no peers array")?;
//
//     if peers.is_empty() {
//         return Err("no peers discovered".to_string());
//     }
//
//     let pubkey = peers[0]["public_key"].as_str()
//     .ok_or("no public_key field")?
//     .to_string();
//
//     Ok(pubkey)
// }
//
// fn trust_peer(netns: &str, peer_pubkey: &str) -> Result<(), String> {
//     let result = Command::new("ip")
//     .args(&["netns", "exec", netns])
//     .args(&["curl", "-s", "-X", "POST", "http://127.0.0.1:9001/api/trust/add",
//           "-H", "Content-Type: application/json",
//           "-d", &format!(r#"{{"public_key":"{}"}}"#, peer_pubkey)])
//     .output()
//     .map_err(|e| format!("trust curl failed: {}", e))?;
//
//     if !result.status.success() {
//         return Err(format!("trust failed: {}", String::from_utf8_lossy(&result.stderr)));
//     }
//
//     Ok(())
// }
//
// #[test]
// fn test_file_transfer_two_nodes() {
//     if !netns_available() {
//         eprintln!("SKIP: netns not available");
//         return;
//     }
//     cleanup_summitd();
//
//     if !summitd_path().exists() || !summit_ctl_path().exists() {
//         eprintln!("SKIP: binaries not built (run: cargo build -p summitd -p summit-ctl)");
//         return;
//     }
//
//     // Create test file
//     let test_content = "Integration test file transfer via Summit Protocol";
//     std::fs::write("/tmp/test-integration.txt", test_content).unwrap();
//
//     // Start daemons in background
//     let mut node_a = Command::new("ip")
//     .args(&["netns", "exec", NS_A])
//     .arg(summitd_path())
//     .arg("veth-a")
//     .env("RUST_LOG", "info")
//     .spawn()
//     .expect("failed to start node A");
//
//     let mut node_b = Command::new("ip")
//     .args(&["netns", "exec", NS_B])
//     .arg(summitd_path())
//     .arg("veth-b")
//     .env("RUST_LOG", "info")
//     .spawn()
//     .expect("failed to start node B");
//
//     // Wait for APIs to be ready
//     println!("Waiting for daemons to start...");
//     wait_for_api("summit-a", 40).expect("summit-a API not ready");
//     wait_for_api("summit-b", 40).expect("summit-b API not ready");
//
//     // Wait for peer discovery
//     println!("Waiting for peer discovery...");
//     thread::sleep(Duration::from_secs(5));
//
//     // Get peer public keys
//     let pubkey_b = get_peer_pubkey("summit-a")
//     .expect("failed to get B's pubkey from A");
//     let pubkey_a = get_peer_pubkey("summit-b")
//     .expect("failed to get A's pubkey from B");
//
//     println!("Node A sees B: {}...", &pubkey_b[..16]);
//     println!("Node B sees A: {}...", &pubkey_a[..16]);
//
//     // Establish mutual trust
//     println!("Establishing mutual trust...");
//     trust_peer("summit-a", &pubkey_b).expect("A failed to trust B");
//     trust_peer("summit-b", &pubkey_a).expect("B failed to trust A");
//
//     // Wait for trust to propagate
//     thread::sleep(Duration::from_secs(2));
//
//     // Verify sessions established
//     let status_result = Command::new("ip")
//     .args(&["netns", "exec", NS_A])
//     .arg(summit_ctl_path())
//     .arg("status")
//     .output()
//     .expect("failed to check status");
//
//     let status_out = String::from_utf8_lossy(&status_result.stdout);
//     println!("Node A status:\n{}", status_out);
//
//     if !status_out.contains("Active sessions  : 1") {
//         eprintln!("WARNING: Session may not be established");
//     }
//
//     // Send file from A to B
//     println!("Sending file from A to B...");
//     let send_result = Command::new("ip")
//     .args(&["netns", "exec", NS_A])
//     .arg(summit_ctl_path())
//     .args(&["send", "/tmp/test-integration.txt"])
//     .output()
//     .expect("failed to send file");
//
//     let send_stdout = String::from_utf8_lossy(&send_result.stdout);
//     let send_stderr = String::from_utf8_lossy(&send_result.stderr);
//
//     if !send_result.status.success() {
//         eprintln!("Send failed!");
//         eprintln!("stdout: {}", send_stdout);
//         eprintln!("stderr: {}", send_stderr);
//         panic!("send command failed");
//     }
//
//     println!("Send output: {}", send_stdout);
//     assert!(send_stdout.contains("File queued") || send_stdout.contains("Chunks"),
//             "unexpected send output: {}", send_stdout);
//
//     // Wait for transfer
//     println!("Waiting for file transfer...");
//     thread::sleep(Duration::from_secs(10));
//
//     // Check files on B
//     let files_result = Command::new("ip")
//     .args(&["netns", "exec", NS_B])
//     .arg(summit_ctl_path())
//     .arg("files")
//     .output()
//     .expect("failed to list files");
//
//     let files_stdout = String::from_utf8_lossy(&files_result.stdout);
//     println!("Node B files:\n{}", files_stdout);
//
//     assert!(files_stdout.contains("test-integration.txt"),
//             "file not received on B");
//
//     // Verify content
//     let received = std::fs::read_to_string("/tmp/summit-received/test-integration.txt")
//     .expect("received file not found");
//     assert_eq!(received, test_content, "file content mismatch");
//
//     println!("✓ File transfer successful");
//
//     // Cleanup
//     node_a.kill().ok();
//     node_b.kill().ok();
//     thread::sleep(Duration::from_secs(1));
//     std::fs::remove_file("/tmp/test-integration.txt").ok();
//     std::fs::remove_file("/tmp/summit-received/test-integration.txt").ok();
// }
//
// #[test]
// fn test_status_shows_session() {
//     if !netns_available() {
//         eprintln!("SKIP: netns not available");
//         return;
//     }
//     cleanup_summitd();
//
//     if !summitd_path().exists() || !summit_ctl_path().exists() {
//         eprintln!("SKIP: binaries not built");
//         return;
//     }
//
//     // Start both nodes
//     let mut node_a = Command::new("ip")
//     .args(&["netns", "exec", NS_A])
//     .arg(summitd_path())
//     .arg("veth-a")
//     .env("RUST_LOG", "info")
//     .spawn()
//     .expect("failed to start node A");
//
//     let mut node_b = Command::new("ip")
//     .args(&["netns", "exec", NS_B])
//     .arg(summitd_path())
//     .arg("veth-b")
//     .env("RUST_LOG", "info")
//     .spawn()
//     .expect("failed to start node B");
//
//     // Wait for APIs
//     println!("Waiting for daemons...");
//     wait_for_api("summit-a", 40).expect("summit-a API not ready");
//     wait_for_api("summit-b", 40).expect("summit-b API not ready");
//
//     // Wait for discovery and session
//     thread::sleep(Duration::from_secs(6));
//
//     // Check status on A
//     let status_result = Command::new("ip")
//     .args(&["netns", "exec", NS_A])
//     .arg(summit_ctl_path())
//     .arg("status")
//     .output()
//     .expect("failed to get status");
//
//     let status_stdout = String::from_utf8_lossy(&status_result.stdout);
//     println!("Node A status:\n{}", status_stdout);
//
//     assert!(status_stdout.contains("Peers discovered :"), "status missing peers line");
//     assert!(status_stdout.contains("Active sessions"), "status missing sessions line");
//
//     // Check status on B
//     let status_result_b = Command::new("ip")
//     .args(&["netns", "exec", NS_B])
//     .arg(summit_ctl_path())
//     .arg("status")
//     .output()
//     .expect("failed to get status from B");
//
//     let status_stdout_b = String::from_utf8_lossy(&status_result_b.stdout);
//     println!("Node B status:\n{}", status_stdout_b);
//
//     println!("✓ Status command works on both nodes");
//
//     // Cleanup
//     node_a.kill().ok();
//     node_b.kill().ok();
// }
//
// #[test]
// fn test_trust_system() {
//     if !netns_available() {
//         eprintln!("SKIP: netns not available");
//         return;
//     }
//     cleanup_summitd();
//
//     if !summitd_path().exists() || !summit_ctl_path().exists() {
//         eprintln!("SKIP: binaries not built");
//         return;
//     }
//
//     // Start both nodes
//     let mut node_a = Command::new("ip")
//     .args(&["netns", "exec", NS_A])
//     .arg(summitd_path())
//     .arg("veth-a")
//     .env("RUST_LOG", "info")
//     .spawn()
//     .expect("failed to start node A");
//
//     let mut node_b = Command::new("ip")
//     .args(&["netns", "exec", NS_B])
//     .arg(summitd_path())
//     .arg("veth-b")
//     .env("RUST_LOG", "info")
//     .spawn()
//     .expect("failed to start node B");
//
//     // Wait for APIs
//     wait_for_api("summit-a", 40).expect("summit-a API not ready");
//     wait_for_api("summit-b", 40).expect("summit-b API not ready");
//
//     // Wait for discovery
//     thread::sleep(Duration::from_secs(5));
//
//     // Get peer pubkey
//     let pubkey_b = get_peer_pubkey("summit-a")
//     .expect("failed to get B's pubkey");
//
//     println!("B's pubkey: {}...", &pubkey_b[..16]);
//
//     // Trust peer on A
//     trust_peer("summit-a", &pubkey_b).expect("failed to trust B");
//
//     // Verify trust list
//     let trust_result = Command::new("ip")
//     .args(&["netns", "exec", NS_A])
//     .arg(summit_ctl_path())
//     .args(&["trust", "list"])
//     .output()
//     .expect("failed to list trust");
//
//     let trust_stdout = String::from_utf8_lossy(&trust_result.stdout);
//     println!("Trust list:\n{}", trust_stdout);
//
//     assert!(trust_stdout.contains("Trusted") || trust_stdout.contains(&pubkey_b[..16]),
//             "peer not in trust list");
//
//     println!("✓ Trust system works");
//
//     // Cleanup
//     node_a.kill().ok();
//     node_b.kill().ok();
// }
