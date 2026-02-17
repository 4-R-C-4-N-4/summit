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

use std::process::Command;
use anyhow::{Context, Result, bail};

// ── Harness ───────────────────────────────────────────────────────────────────

/// The two namespace names used throughout tests.
pub const NS_A: &str = "summit-a";
pub const NS_B: &str = "summit-b";
pub const VETH_A: &str = "a-veth";
pub const VETH_B: &str = "b-veth";
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

    let out_a = netns_exec(NS_A, &["ip", "link", "show", VETH_A])
        .expect("veth-a should exist in summit-a");
    assert!(out_a.contains(VETH_A), "veth-a not found in summit-a");

    let out_b = netns_exec(NS_B, &["ip", "link", "show", VETH_B])
        .expect("veth-b should exist in summit-b");
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

    let addr_a = link_local_addr(NS_A, VETH_A)
        .expect("summit-a should have a link-local address");
    let addr_b = link_local_addr(NS_B, VETH_B)
        .expect("summit-b should have a link-local address");

    println!("summit-a: {addr_a}");
    println!("summit-b: {addr_b}");

    assert!(addr_a.starts_with("fe80::"), "expected link-local address in summit-a");
    assert!(addr_b.starts_with("fe80::"), "expected link-local address in summit-b");
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
    let addr_b_raw = link_local_addr(NS_B, VETH_B)
    .expect("summit-b should have a link-local address");
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
        Err(e)  => panic!("ping6 from summit-a to summit-b failed: {e}"),
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
    let addr_a_raw = link_local_addr(NS_A, VETH_A)
    .expect("summit-a should have a link-local address");
    let addr_a = addr_a_raw
    .split('%')
    .next()
    .map(|a| format!("{a}%{VETH_B}"))
    .unwrap();

    println!("Pinging {addr_a} from summit-b...");
    let result = netns_exec(NS_B, &["ping", "-6", "-c", "3", "-W", "2", &addr_a]);
    match &result {
        Ok(out) => println!("{out}"),
        Err(e)  => panic!("ping6 from summit-b to summit-a failed: {e}"),
    }
    assert!(result.is_ok());
}

// ── File Transfer Tests ───────────────────────────────────────────────────────

use std::time::Duration;
use std::thread;

fn summitd_path() -> std::path::PathBuf {
    std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent().unwrap()
        .parent().unwrap()
        .join("target/debug/summitd")
}

fn summit_ctl_path() -> std::path::PathBuf {
    std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent().unwrap()
        .parent().unwrap()
        .join("target/debug/summit-ctl")
}

#[test]
fn test_file_transfer_two_nodes() {
    if !netns_available() {
        eprintln!("SKIP: netns not available");
        return;
    }
    
    if !summitd_path().exists() || !summit_ctl_path().exists() {
        eprintln!("SKIP: binaries not built (run: cargo build -p summitd -p summit-ctl)");
        return;
    }
    
    // Create test file
    let test_content = "Integration test file transfer";
    std::fs::write("/tmp/test-integration.txt", test_content).unwrap();
    
    // Start daemons in background
    let mut node_a = Command::new("ip")
        .args(&["netns", "exec", NS_A])
        .arg(summitd_path())
        .arg("a-veth")
        .env("RUST_LOG", "info")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .expect("failed to start node A");
    
    let mut node_b = Command::new("ip")
        .args(&["netns", "exec", NS_B])
        .arg(summitd_path())
        .arg("b-veth")
        .env("RUST_LOG", "info")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .expect("failed to start node B");
    
    // Wait for session establishment
    thread::sleep(Duration::from_secs(6));
    
    // Verify session established before sending
    let status_check = Command::new("ip")
    .args(&["netns", "exec", NS_A])
    .arg(summit_ctl_path())
    .arg("status")
    .output()
    .expect("failed to check status");

    let status_out = String::from_utf8_lossy(&status_check.stdout);
    if !status_out.contains("Active sessions  : 1") {
        eprintln!("WARNING: No session established yet. Status output:\n{}", status_out);
        thread::sleep(Duration::from_secs(5)); // Wait longer
    }

    // Send file from A
    let send_result = Command::new("ip")
        .args(&["netns", "exec", NS_A])
        .arg(summit_ctl_path())
        .args(&["send", "/tmp/test-integration.txt"])
        .output()
        .expect("failed to send file");
    
    assert!(send_result.status.success(), "send failed: {}", 
            String::from_utf8_lossy(&send_result.stderr));
    
    let send_stdout = String::from_utf8_lossy(&send_result.stdout);
    assert!(send_stdout.contains("File queued"), "unexpected send output: {}", send_stdout);
    
    // Wait for transfer
    thread::sleep(Duration::from_secs(3));
    
    // Check files on B
    let files_result = Command::new("ip")
        .args(&["netns", "exec", NS_B])
        .arg(summit_ctl_path())
        .arg("files")
        .output()
        .expect("failed to list files");
    
    let files_stdout = String::from_utf8_lossy(&files_result.stdout);
    assert!(files_stdout.contains("test-integration.txt"), 
            "file not received: {}", files_stdout);
    
    // Verify content
    let received = std::fs::read_to_string("/tmp/summit-received/test-integration.txt")
        .expect("received file not found");
    assert_eq!(received, test_content);
    
    println!("✓ File transfer successful");
    
    // Cleanup
    node_a.kill().ok();
    node_b.kill().ok();
    std::fs::remove_file("/tmp/test-integration.txt").ok();
    std::fs::remove_file("/tmp/summit-received/test-integration.txt").ok();
}

#[test]
fn test_status_shows_session() {
    if !netns_available() {
        eprintln!("SKIP: netns not available");
        return;
    }
    
    if !summitd_path().exists() || !summit_ctl_path().exists() {
        eprintln!("SKIP: binaries not built");
        return;
    }
    
    // Start both nodes
    let mut node_a = Command::new("ip")
        .args(&["netns", "exec", NS_A])
        .arg(summitd_path())
        .arg("a-veth")
        .env("RUST_LOG", "error")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .expect("failed to start node A");
    
    let mut node_b = Command::new("ip")
        .args(&["netns", "exec", NS_B])
        .arg(summitd_path())
        .arg("b-veth")
        .env("RUST_LOG", "error")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .expect("failed to start node B");
    
    thread::sleep(Duration::from_secs(6));
    
    // Check status
    let status_result = Command::new("ip")
        .args(&["netns", "exec", NS_A])
        .arg(summit_ctl_path())
        .arg("status")
        .output()
        .expect("failed to get status");
    
    let status_stdout = String::from_utf8_lossy(&status_result.stdout);
    assert!(status_stdout.contains("Active sessions  : 1"), "status: {}", status_stdout);
    assert!(status_stdout.contains("Peers discovered : 1"), "status: {}", status_stdout);
    
    println!("✓ Status command works");
    
    // Cleanup
    node_a.kill().ok();
    node_b.kill().ok();
}
