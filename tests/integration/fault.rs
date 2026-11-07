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

/// Apply random packet loss to an interface's EGRESS (outbound) traffic.
/// netem only shapes what `ns` sends: to lose data flowing A -> B, apply it
/// to A. Loss is per IP packet, so a 32 KiB chunk (~23 fragments) is lost
/// far more often than `loss_pct` — like real Wi-Fi frame loss.
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

/// Drop inbound capability announcements (multicast to ff02::1:9000) in a
/// namespace, while unicast keeps working. This is what Android Wi-Fi does to
/// an app that doesn't hold a MulticastLock.
pub fn block_inbound_multicast(ns: &str) -> FaultGuard {
    let rule = |op: &str| -> Vec<String> {
        [
            "ip",
            "netns",
            "exec",
            ns,
            "ip6tables",
            op,
            "INPUT",
            "-d",
            "ff02::1",
            "-p",
            "udp",
            "--dport",
            "9000",
            "-j",
            "DROP",
        ]
        .iter()
        .map(|s| s.to_string())
        .collect()
    };
    let add = rule("-A");
    let status = Command::new(&add[0])
        .args(&add[1..])
        .status()
        .expect("run ip6tables");
    assert!(status.success(), "failed to install multicast drop rule");
    FaultGuard::new(rule("-D"))
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

/// Deterministically drop every `n`th large datagram arriving in `ns`
/// (the 1st, n+1th, ...). Matches first IPv6 fragments only, so it hits
/// data chunks (and large metadata) but never small control traffic:
/// announcements, handshakes, NACKs, FILE_DONE. Dropping the first fragment
/// loses the whole datagram. Unlike netem this guarantees holes, so a test
/// that must exercise recovery always does.
pub fn drop_every_nth_chunk(ns: &str, n: u32) -> FaultGuard {
    let every = n.to_string();
    let rule = |op: &str| -> Vec<String> {
        [
            "ip",
            "netns",
            "exec",
            ns,
            "ip6tables",
            op,
            "INPUT",
            "-p",
            "udp",
            "-m",
            "frag",
            "--fragfirst",
            "-m",
            "statistic",
            "--mode",
            "nth",
            "--every",
            &every,
            "--packet",
            "0",
            "-j",
            "DROP",
        ]
        .iter()
        .map(|s| s.to_string())
        .collect()
    };
    let add = rule("-A");
    let status = Command::new(&add[0])
        .args(&add[1..])
        .status()
        .expect("run ip6tables");
    assert!(status.success(), "failed to install nth-chunk drop rule");
    FaultGuard::new(rule("-D"))
}

/// Datagrams dropped so far by `drop_every_nth_chunk` rules in `ns`.
pub fn dropped_chunks(ns: &str) -> u64 {
    let out = Command::new("ip")
        .args([
            "netns",
            "exec",
            ns,
            "ip6tables",
            "-L",
            "INPUT",
            "-v",
            "-n",
            "-x",
        ])
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).to_string())
        .unwrap_or_default();
    out.lines()
        .filter(|l| l.contains("statistic mode nth"))
        .filter_map(|l| l.split_whitespace().next()?.parse::<u64>().ok())
        .sum()
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

/// Wait until `path` holds exactly `expected`, failing after `timeout_secs`.
/// Polls, so passing tests finish as soon as the file lands.
pub fn wait_for_file(path: &str, expected: &[u8], timeout_secs: u64) -> anyhow::Result<()> {
    wait_for_condition(timeout_secs, || {
        std::fs::read(path).map(|d| d == expected).unwrap_or(false)
    })
    .map_err(|_| {
        let got = std::fs::read(path)
            .map(|d| format!("{} bytes, content differs", d.len()))
            .unwrap_or_else(|_| "missing".into());
        anyhow::anyhow!("{path} not received intact within {timeout_secs}s ({got})")
    })
}

/// Number of in-progress (incomplete) file assemblies on `ns`.
pub fn files_in_progress(ns: &str) -> usize {
    api_get(ns, "/files")
        .ok()
        .and_then(|f| f["in_progress"].as_array().map(|a| a.len()))
        .unwrap_or(0)
}
