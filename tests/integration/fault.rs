use crate::*;
use std::process::Command;
use std::time::Instant;

// ── FaultGuard — Drop-based cleanup ─────────────────────────────────────────

/// Guarantees cleanup even on test panic (Rust runs `Drop` during stack unwinding).
/// Each fault helper returns a `FaultGuard` that the test binds to a named variable.
pub struct FaultGuard {
    cleanup_args: Vec<String>,
}

impl FaultGuard {
    pub fn new(cleanup_args: Vec<String>) -> Self {
        Self { cleanup_args }
    }
}

impl Drop for FaultGuard {
    fn drop(&mut self) {
        if let Some((cmd, args)) = self.cleanup_args.split_first() {
            let _ = Command::new(cmd).args(args).output();
        }
    }
}

// ── Network fault helpers ───────────────────────────────────────────────────

/// Apply packet loss to an interface inside a namespace.
/// Uses `tc qdisc replace` (idempotent — won't fail if a previous test left a rule).
pub fn add_packet_loss(ns: &str, iface: &str, loss_pct: u32) -> FaultGuard {
    let loss = format!("{}%", loss_pct);
    let _ = Command::new("ip")
        .args([
            "netns", "exec", ns, "tc", "qdisc", "replace", "dev", iface, "root", "netem", "loss",
            &loss,
        ])
        .output();

    FaultGuard::new(vec![
        "ip".into(),
        "netns".into(),
        "exec".into(),
        ns.into(),
        "tc".into(),
        "qdisc".into(),
        "del".into(),
        "dev".into(),
        iface.into(),
        "root".into(),
    ])
}

/// Apply delay (and optional jitter) to an interface inside a namespace.
pub fn add_delay(ns: &str, iface: &str, delay_ms: u32, jitter_ms: u32) -> FaultGuard {
    let delay = format!("{}ms", delay_ms);
    let jitter = format!("{}ms", jitter_ms);
    let _ = Command::new("ip")
        .args([
            "netns", "exec", ns, "tc", "qdisc", "replace", "dev", iface, "root", "netem", "delay",
            &delay, &jitter,
        ])
        .output();

    FaultGuard::new(vec![
        "ip".into(),
        "netns".into(),
        "exec".into(),
        ns.into(),
        "tc".into(),
        "qdisc".into(),
        "del".into(),
        "dev".into(),
        iface.into(),
        "root".into(),
    ])
}

/// Block a specific UDP port via ip6tables inside a namespace.
pub fn block_udp_port(ns: &str, port: u16) -> FaultGuard {
    let port_str = port.to_string();
    let _ = Command::new("ip")
        .args([
            "netns",
            "exec",
            ns,
            "ip6tables",
            "-A",
            "INPUT",
            "-p",
            "udp",
            "--dport",
            &port_str,
            "-j",
            "DROP",
        ])
        .output();

    FaultGuard::new(vec![
        "ip".into(),
        "netns".into(),
        "exec".into(),
        ns.into(),
        "ip6tables".into(),
        "-D".into(),
        "INPUT".into(),
        "-p".into(),
        "udp".into(),
        "--dport".into(),
        port_str,
        "-j".into(),
        "DROP".into(),
    ])
}

/// Block all UDP traffic via ip6tables inside a namespace.
pub fn block_all_udp(ns: &str) -> FaultGuard {
    let _ = Command::new("ip")
        .args([
            "netns",
            "exec",
            ns,
            "ip6tables",
            "-A",
            "INPUT",
            "-p",
            "udp",
            "-j",
            "DROP",
        ])
        .output();

    FaultGuard::new(vec![
        "ip".into(),
        "netns".into(),
        "exec".into(),
        ns.into(),
        "ip6tables".into(),
        "-D".into(),
        "INPUT".into(),
        "-p".into(),
        "udp".into(),
        "-j".into(),
        "DROP".into(),
    ])
}

/// Bring a network interface down inside a namespace.
pub fn link_down(ns: &str, iface: &str) -> FaultGuard {
    let _ = Command::new("ip")
        .args([
            "netns", "exec", ns, "ip", "link", "set", "dev", iface, "down",
        ])
        .output();

    FaultGuard::new(vec![
        "ip".into(),
        "netns".into(),
        "exec".into(),
        ns.into(),
        "ip".into(),
        "link".into(),
        "set".into(),
        "dev".into(),
        iface.into(),
        "up".into(),
    ])
}

/// Send garbage UDP packets to a port inside a namespace.
/// No cleanup needed — fire and forget.
pub fn send_garbage_udp(ns: &str, port: u16, count: u32) {
    let script = format!(
        "import socket; s=socket.socket(socket.AF_INET6, socket.SOCK_DGRAM); [s.sendto(b'\\x00'*64, ('::1', {})) for _ in range({})]; s.close()",
        port, count
    );
    let _ = Command::new("ip")
        .args(["netns", "exec", ns, "python3", "-c", &script])
        .output();
}

// ── Invariant helpers ───────────────────────────────────────────────────────

/// Check if the daemon API is reachable in a namespace.
pub fn daemon_alive(ns: &str) -> bool {
    wait_for_api(ns, 3).is_ok()
}

/// Query `/status` and return the number of active sessions.
pub fn session_count(ns: &str) -> usize {
    match api_get(ns, "/status") {
        Ok(status) => status["sessions"].as_array().map(|a| a.len()).unwrap_or(0),
        Err(_) => 0,
    }
}

/// Generic polling helper. Calls `poll_fn` every 500ms until it returns `true`
/// or `timeout_secs` expires.
pub fn wait_for_condition<F>(timeout_secs: u64, poll_fn: F) -> anyhow::Result<()>
where
    F: Fn() -> bool,
{
    let start = Instant::now();
    let timeout = Duration::from_secs(timeout_secs);
    while start.elapsed() < timeout {
        if poll_fn() {
            return Ok(());
        }
        thread::sleep(Duration::from_millis(500));
    }
    anyhow::bail!("condition not met within {}s", timeout_secs)
}
