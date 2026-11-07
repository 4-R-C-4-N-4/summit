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

mod compute;
mod failures;
#[allow(dead_code)]
mod fault;
mod files;
mod infra;
mod messaging;
mod one_way;
mod recovery;
mod service_config;
mod sessions;
mod status;
mod trust;

// ── Constants ─────────────────────────────────────────────────────────────────

pub const NS_A: &str = "summit-a";
pub const NS_B: &str = "summit-b";
pub const VETH_A: &str = "veth-a";
pub const VETH_B: &str = "veth-b";

/// Deterministic non-repeating bytes (xorshift). Use for transfer tests:
/// files with repeated blocks dedupe to fewer unique chunks, which would
/// hide whether multi-block recovery actually works.
pub fn distinct_bytes(len: usize, seed: u32) -> Vec<u8> {
    let mut x = seed | 1;
    (0..len)
        .map(|_| {
            x ^= x << 13;
            x ^= x >> 17;
            x ^= x << 5;
            x as u8
        })
        .collect()
}

/// Serializes all daemon-based tests so they don't conflict on port 9001.
pub static DAEMON_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// Take the daemon lock. A test that panics while holding it poisons the
/// mutex; that must not turn every later test into a bogus failure.
pub fn daemon_lock() -> std::sync::MutexGuard<'static, ()> {
    DAEMON_LOCK.lock().unwrap_or_else(|e| e.into_inner())
}

// ── Binary paths ──────────────────────────────────────────────────────────────

/// Debug build dir, honouring CARGO_TARGET_DIR like cargo itself does.
fn debug_dir() -> PathBuf {
    std::env::var_os("CARGO_TARGET_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .parent()
                .unwrap()
                .parent()
                .unwrap()
                .join("target")
        })
        .join("debug")
}

pub fn summitd_path() -> PathBuf {
    debug_dir().join("summitd")
}

pub fn summit_ctl_path() -> PathBuf {
    debug_dir().join("summit-ctl")
}

pub fn binaries_available() -> bool {
    summitd_path().exists() && summit_ctl_path().exists()
}

// ── Process helpers ───────────────────────────────────────────────────────────

/// Kill the test daemons to ensure clean test state. Matches the full binary
/// path so a real summitd running on this machine is left alone.
pub fn cleanup_summitd() {
    Command::new("pkill")
        .arg("-9")
        .arg("-f")
        .arg(summitd_path())
        .output()
        .ok();
    thread::sleep(Duration::from_millis(500));
}

/// Spawn a summitd in the given namespace on the given interface.
/// `extra_env` is a list of (key, value) pairs appended to the environment.
pub fn spawn_daemon(ns: &str, iface: &str, extra_env: &[(&str, &str)]) -> Child {
    spawn_daemon_inner(ns, iface, extra_env, None)
}

/// Like `spawn_daemon`, but with a fixed identity (X25519 private key), for
/// tests that depend on which peer has the lower public key.
pub fn spawn_daemon_with_key(
    ns: &str,
    iface: &str,
    extra_env: &[(&str, &str)],
    private_key: [u8; 32],
) -> Child {
    spawn_daemon_inner(ns, iface, extra_env, Some(private_key))
}

fn spawn_daemon_inner(
    ns: &str,
    iface: &str,
    extra_env: &[(&str, &str)],
    private_key: Option<[u8; 32]>,
) -> Child {
    let mut cmd = Command::new("ip");
    cmd.args(["netns", "exec", ns]);
    cmd.arg(summitd_path());
    cmd.arg(iface);
    cmd.env("RUST_LOG", "info");

    // Every spawn gets its own config/data roots. The keypair is persisted
    // under the config dir, so sharing it would give both daemons the same
    // identity and they would ignore each other's announcements as "self".
    // A fresh root per spawn also means a restarted daemon has a new key.
    static SPAWN_SEQ: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
    let seq = SPAWN_SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let root = format!("/tmp/summit-it-{}-{}-{}", std::process::id(), ns, seq);
    cmd.env("XDG_CONFIG_HOME", format!("{root}/config"));
    cmd.env("XDG_DATA_HOME", format!("{root}/data"));
    cmd.env("SUMMIT_CACHE", format!("{root}/cache"));
    cmd.env("SUMMIT_CONFIG", format!("{root}/config.toml"));
    if let Some(key) = private_key {
        // Default keypair path is $XDG_CONFIG_HOME/summit/keypair.
        let dir = format!("{root}/config/summit");
        std::fs::create_dir_all(&dir).expect("create keypair dir");
        std::fs::write(format!("{dir}/keypair"), key).expect("write keypair");
    }
    // File tests look for received files here (the pre-XDG default).
    cmd.env("SUMMIT_FILE_TRANSFER__STORAGE_PATH", "/tmp/summit-received");
    for (k, v) in extra_env {
        cmd.env(k, v);
    }
    cmd.spawn().expect("failed to spawn summitd")
}

/// Wait until the REST API is reachable inside a namespace.
/// Polls up to `max_attempts * 500 ms`.
pub fn wait_for_api(ns: &str, max_attempts: u32) -> Result<()> {
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
pub fn api_get(ns: &str, path: &str) -> Result<Value> {
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
pub fn api_post(ns: &str, path: &str, body: &str) -> Result<Value> {
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
pub fn get_peer_pubkey(ns: &str) -> Result<String> {
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
pub fn trust_peer(ns: &str, peer_pubkey: &str) -> Result<usize> {
    let body = serde_json::json!({ "public_key": peer_pubkey }).to_string();
    let resp = api_post(ns, "/trust/add", &body)?;
    Ok(resp["flushed_chunks"].as_u64().unwrap_or(0) as usize)
}

/// Run summit-ctl inside a namespace with given args.
pub fn ctl(ns: &str, args: &[&str]) -> Result<String> {
    let ctl = summit_ctl_path();
    let ctl_str = ctl.to_str().unwrap();
    let mut full_args = vec![ctl_str];
    full_args.extend_from_slice(args);
    netns_exec(ns, &full_args)
}

/// Run summit-ctl inside a namespace, allowing non-zero exit.
pub fn ctl_raw(ns: &str, args: &[&str]) -> std::process::Output {
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
/// Returns false (test skips) when the namespaces or binaries are missing,
/// so a plain `cargo test` on a dev box doesn't fail. With
/// `SUMMIT_REQUIRE_INTEGRATION=1` (set by integration-rootless.sh, i.e. in
/// CI) a missing environment is a failure: a suite that silently skips
/// every test must never look green.
pub fn skip_unless_ready() -> bool {
    let Some(reason) = skip_reason() else {
        return true;
    };
    if std::env::var_os("SUMMIT_REQUIRE_INTEGRATION").is_some() {
        panic!("integration environment required but unavailable: {reason}");
    }
    eprintln!("SKIP: {reason}");
    false
}

fn skip_reason() -> Option<&'static str> {
    if !netns_available() {
        return Some("netns not available — run scripts/integration-rootless.sh");
    }
    if !binaries_available() {
        return Some("binaries not built — run: cargo build -p summitd -p summit-ctl");
    }
    None
}

/// Wait for sessions to be established on NS_A (returns the first session_id).
pub fn wait_for_session(secs: u64) -> Result<String> {
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
