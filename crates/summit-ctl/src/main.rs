//! summit-ctl — command-line interface for the Summit daemon.

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

const DEFAULT_PORT: u16 = 9001;

// ── Response types ────────────────────────────────────────────────────────────

#[derive(Deserialize)]
struct StatusResponse {
    sessions: Vec<SessionInfo>,
    cache: CacheInfo,
    peers_discovered: usize,
}

#[derive(Deserialize)]
#[allow(dead_code)]
struct SessionInfo {
    session_id: String,
    peer: String,
    peer_pubkey: String,
    contract: String,
    chunk_port: u16,
    established_secs: u64,
    trust_level: String,
}

#[derive(Deserialize)]
struct PeersResponse {
    peers: Vec<PeerInfo>,
}

#[derive(Deserialize)]
#[allow(dead_code)]
struct PeerInfo {
    public_key: String,
    addr: String,
    session_port: u16,
    services: Vec<String>,
    service_count: usize,
    is_complete: bool,
    version: u32,
    last_seen_secs: u64,
    trust_level: String,
    buffered_chunks: usize,
}

#[derive(Deserialize)]
struct CacheInfo {
    chunks: usize,
    bytes: u64,
}

#[derive(Deserialize)]
struct ClearResponse {
    cleared: usize,
}

#[derive(Deserialize)]
struct TrustListResponse {
    rules: Vec<TrustRule>,
}

#[derive(Deserialize)]
struct TrustRule {
    public_key: String,
    level: String,
}

#[derive(Serialize)]
struct TrustAddRequest {
    public_key: String,
}

#[derive(Deserialize)]
struct TrustAddResponse {
    public_key: String,
    flushed_chunks: usize,
}

#[derive(Serialize)]
struct TrustBlockRequest {
    public_key: String,
}

#[derive(Deserialize)]
struct TrustBlockResponse {
    public_key: String,
}

#[derive(Deserialize)]
struct TrustPendingResponse {
    peers: Vec<PendingPeer>,
}

#[derive(Deserialize)]
struct PendingPeer {
    public_key: String,
    buffered_chunks: usize,
}

#[derive(Deserialize)]
struct SendResponse {
    filename: String,
    bytes: u64,
    chunks_sent: usize,
}

#[derive(Deserialize)]
struct FilesResponse {
    received: Vec<String>,
    in_progress: Vec<String>,
}

#[derive(Deserialize)]
struct ServicesResponse {
    services: Vec<ServiceStatus>,
}

#[derive(Deserialize)]
struct ServiceStatus {
    name: String,
    enabled: bool,
    contract: String,
}

#[derive(Deserialize)]
struct ComputeTasksResponse {
    peer_pubkey: String,
    tasks: Vec<ComputeTaskJson>,
}

#[derive(Deserialize)]
#[allow(dead_code)]
struct ComputeTaskJson {
    task_id: String,
    status: String,
    submitted_at: u64,
    updated_at: u64,
    result: Option<serde_json::Value>,
    elapsed_ms: Option<u64>,
}

#[derive(Serialize)]
struct ComputeSubmitRequest {
    to: String,
    payload: serde_json::Value,
}

#[derive(Deserialize)]
struct ComputeSubmitResponse {
    task_id: String,
    timestamp: u64,
}

// ── HTTP helpers ──────────────────────────────────────────────────────────────

fn base_url(port: u16) -> String {
    format!("http://127.0.0.1:{}/api", port)
}

async fn get_json<T: for<'de> Deserialize<'de>>(url: &str) -> Result<T> {
    reqwest::get(url)
        .await
        .with_context(|| format!("failed to connect to summitd at {} — is it running?", url))?
        .json::<T>()
        .await
        .context("failed to parse response")
}

async fn post_json<T: for<'de> Deserialize<'de>>(url: &str) -> Result<T> {
    reqwest::Client::new()
        .post(url)
        .send()
        .await
        .with_context(|| format!("failed to connect to summitd at {} — is it running?", url))?
        .json::<T>()
        .await
        .context("failed to parse response")
}

async fn post_json_body<T, R>(url: &str, body: &T) -> Result<R>
where
    T: Serialize,
    R: for<'de> Deserialize<'de>,
{
    reqwest::Client::new()
        .post(url)
        .json(body)
        .send()
        .await
        .with_context(|| format!("failed to connect to summitd at {} — is it running?", url))?
        .json::<R>()
        .await
        .context("failed to parse response")
}

// ── Subcommand handlers ───────────────────────────────────────────────────────

async fn cmd_status(port: u16) -> Result<()> {
    let resp: StatusResponse = get_json(&format!("{}/status", base_url(port))).await?;

    println!("═══════════════════════════════════════");
    println!("  Summit Daemon Status");
    println!("═══════════════════════════════════════");
    println!("  Peers discovered : {}", resp.peers_discovered);
    println!("  Active sessions  : {}", resp.sessions.len());
    println!("  Cache chunks     : {}", resp.cache.chunks);
    println!("  Cache size       : {} bytes", resp.cache.bytes);

    if resp.sessions.is_empty() {
        println!("\n  No active sessions.");
    } else {
        println!("\n  Sessions:");
        for s in &resp.sessions {
            let trust_icon = match s.trust_level.as_str() {
                "Trusted" => "✓",
                "Blocked" => "✗",
                _ => "?",
            };
            println!("  ┌─ {} {}", trust_icon, &s.session_id[..16]);
            println!("  │  peer     : {}", s.peer);
            println!("  │  pubkey   : {}", &s.peer_pubkey);
            println!("  │  contract : {}", s.contract);
            println!("  │  trust    : {}", s.trust_level);
            println!("  └─ uptime   : {}s", s.established_secs);
        }
    }

    Ok(())
}

async fn cmd_peers(port: u16) -> Result<()> {
    let resp: PeersResponse = get_json(&format!("{}/peers", base_url(port))).await?;

    if resp.peers.is_empty() {
        println!("No peers discovered yet.");
        return Ok(());
    }

    println!("═══════════════════════════════════════");
    println!("  Discovered Peers ({})", resp.peers.len());
    println!("═══════════════════════════════════════");

    for p in &resp.peers {
        let trust_icon = match p.trust_level.as_str() {
            "Trusted" => "✓",
            "Blocked" => "✗",
            _ => "?",
        };

        let complete_marker = if p.is_complete { "" } else { " (incomplete)" };

        println!("  ┌─ {} {}", trust_icon, &p.public_key);
        println!("  │  addr         : {}", p.addr);
        println!("  │  session port : {}", p.session_port);
        println!(
            "  │  services     : {}/{}{} — [{}]",
            p.services.len(),
            p.service_count,
            complete_marker,
            p.services
                .iter()
                .map(|s| s[..8].to_string())
                .collect::<Vec<_>>()
                .join(", ")
        );
        println!("  │  trust        : {}", p.trust_level);
        if p.buffered_chunks > 0 {
            println!("  │  buffered     : {} chunks", p.buffered_chunks);
        }
        println!("  └─ last seen    : {}s ago", p.last_seen_secs);
    }

    Ok(())
}

async fn cmd_cache(port: u16) -> Result<()> {
    let resp: CacheInfo = get_json(&format!("{}/cache", base_url(port))).await?;

    println!("═══════════════════════════════════════");
    println!("  Cache Stats");
    println!("═══════════════════════════════════════");
    println!("  Chunks : {}", resp.chunks);
    println!(
        "  Bytes  : {} ({:.1} KB)",
        resp.bytes,
        resp.bytes as f64 / 1024.0
    );

    Ok(())
}

async fn cmd_cache_clear(port: u16) -> Result<()> {
    let resp: ClearResponse = post_json(&format!("{}/cache/clear", base_url(port))).await?;
    println!("Cleared {} chunks from cache.", resp.cleared);
    Ok(())
}

async fn cmd_send(
    port: u16,
    path: &str,
    target_peer: Option<&str>,
    target_session: Option<&str>,
) -> Result<()> {
    use reqwest::multipart;

    let file_data =
        std::fs::read(path).with_context(|| format!("failed to read file: {}", path))?;

    let filename = std::path::Path::new(path)
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("file")
        .to_string();

    let part = multipart::Part::bytes(file_data).file_name(filename.clone());

    // Build target JSON
    let target_json = if let Some(peer) = target_peer {
        serde_json::json!({
            "type": "peer",
            "public_key": peer
        })
    } else if let Some(session) = target_session {
        serde_json::json!({
            "type": "session",
            "session_id": session
        })
    } else {
        serde_json::json!({
            "type": "broadcast"
        })
    };

    let target_part =
        multipart::Part::text(target_json.to_string()).mime_str("application/json")?;

    let form = multipart::Form::new()
        .part("file", part)
        .part("target", target_part);

    let client = reqwest::Client::new();
    let resp: SendResponse = client
        .post(format!("{}/send", base_url(port)))
        .multipart(form)
        .send()
        .await
        .context("failed to send file to daemon")?
        .json()
        .await
        .context("failed to parse send response")?;

    let target_desc = if target_peer.is_some() {
        "to peer"
    } else if target_session.is_some() {
        "to session"
    } else {
        "to all trusted peers (broadcast)"
    };

    println!("File queued for sending {}:", target_desc);
    println!("  Filename : {}", resp.filename);
    println!("  Bytes    : {}", resp.bytes);
    println!("  Chunks   : {}", resp.chunks_sent);

    Ok(())
}

async fn cmd_files(port: u16) -> Result<()> {
    let resp: FilesResponse = get_json(&format!("{}/files", base_url(port))).await?;

    if resp.received.is_empty() && resp.in_progress.is_empty() {
        println!("No files received yet.");
        return Ok(());
    }

    println!("═══════════════════════════════════════");
    println!("  Received Files");
    println!("═══════════════════════════════════════");

    if resp.received.is_empty() {
        println!("  (none)");
    } else {
        for file in &resp.received {
            println!("  ✓ {}", file);
        }
    }

    if !resp.in_progress.is_empty() {
        println!("\n  In Progress:");
        for file in &resp.in_progress {
            println!("  ⋯ {}", file);
        }
    }

    Ok(())
}

async fn cmd_trust_list(port: u16) -> Result<()> {
    let resp: TrustListResponse = get_json(&format!("{}/trust", base_url(port))).await?;

    if resp.rules.is_empty() {
        println!("No explicit trust rules. All peers default to Untrusted.");
        return Ok(());
    }

    println!("═══════════════════════════════════════");
    println!("  Trust Rules ({})", resp.rules.len());
    println!("═══════════════════════════════════════");

    for rule in &resp.rules {
        let icon = match rule.level.as_str() {
            "Trusted" => "✓",
            "Blocked" => "✗",
            _ => "?",
        };
        println!("  {} {} — {}", icon, &rule.public_key, rule.level);
    }

    Ok(())
}

async fn cmd_trust_add(port: u16, pubkey: &str) -> Result<()> {
    let req = TrustAddRequest {
        public_key: pubkey.to_string(),
    };

    let resp: TrustAddResponse =
        post_json_body(&format!("{}/trust/add", base_url(port)), &req).await?;

    println!("✓ Peer trusted: {}", &resp.public_key[..16]);
    if resp.flushed_chunks > 0 {
        println!("  Processed {} buffered chunks", resp.flushed_chunks);
    }

    Ok(())
}

async fn cmd_trust_block(port: u16, pubkey: &str) -> Result<()> {
    let req = TrustBlockRequest {
        public_key: pubkey.to_string(),
    };

    let resp: TrustBlockResponse =
        post_json_body(&format!("{}/trust/block", base_url(port)), &req).await?;

    println!("✗ Peer blocked: {}", &resp.public_key[..16]);

    Ok(())
}

async fn cmd_trust_pending(port: u16) -> Result<()> {
    let resp: TrustPendingResponse = get_json(&format!("{}/trust/pending", base_url(port))).await?;

    if resp.peers.is_empty() {
        println!("No buffered chunks from untrusted peers.");
        return Ok(());
    }

    println!("═══════════════════════════════════════");
    println!("  Untrusted Peers with Buffered Chunks");
    println!("═══════════════════════════════════════");

    for peer in &resp.peers {
        println!(
            "  ? {} — {} chunks buffered",
            &peer.public_key[..16],
            peer.buffered_chunks
        );
    }

    println!(
        "\nUse 'summit-ctl trust add <pubkey>' to trust a peer and process their buffered chunks."
    );

    Ok(())
}

// Add these new command handlers:

async fn cmd_shutdown(port: u16) -> Result<()> {
    #[derive(Deserialize)]
    struct ShutdownResponse {
        message: String,
    }

    let resp: ShutdownResponse = post_json(&format!("{}/daemon/shutdown", base_url(port))).await?;
    println!("{}", resp.message);
    Ok(())
}

async fn cmd_session_drop(port: u16, session_id: &str) -> Result<()> {
    #[derive(Deserialize)]
    struct DropResponse {
        session_id: String,
        dropped: bool,
    }

    let resp: DropResponse = reqwest::Client::new()
        .delete(format!("{}/sessions/{}", base_url(port), session_id))
        .send()
        .await
        .context("failed to drop session")?
        .json()
        .await
        .context("failed to parse response")?;

    if resp.dropped {
        println!("✓ Session dropped: {}...", &resp.session_id[..16]);
    } else {
        println!("Session not found: {}", session_id);
    }

    Ok(())
}

async fn cmd_session_inspect(port: u16, session_id: &str) -> Result<()> {
    #[derive(Deserialize)]
    struct InspectResponse {
        session_id: String,
        peer_addr: String,
        peer_pubkey: String,
        contract: String,
        chunk_port: u16,
        uptime_secs: u64,
        trust_level: String,
    }

    let resp: InspectResponse =
        get_json(&format!("{}/sessions/{}", base_url(port), session_id)).await?;

    println!("═══════════════════════════════════════");
    println!("  Session Details");
    println!("═══════════════════════════════════════");
    println!("  ID       : {}", resp.session_id);
    println!("  Peer     : {}", resp.peer_addr);
    println!("  Pubkey   : {}", resp.peer_pubkey);
    println!("  Contract : {}", resp.contract);
    println!("  Port     : {}", resp.chunk_port);
    println!("  Uptime   : {}s", resp.uptime_secs);
    println!("  Trust    : {}", resp.trust_level);

    Ok(())
}

async fn cmd_schema_list(port: u16) -> Result<()> {
    #[derive(Deserialize)]
    struct SchemaListResponse {
        schemas: Vec<SchemaItem>,
    }

    #[derive(Deserialize)]
    struct SchemaItem {
        id: String,
        name: String,
        type_tag: u8,
    }

    let resp: SchemaListResponse = get_json(&format!("{}/schema", base_url(port))).await?;

    println!("═══════════════════════════════════════");
    println!("  Known Schemas ({})", resp.schemas.len());
    println!("═══════════════════════════════════════");

    for schema in &resp.schemas {
        println!("  ┌─ {} (tag: {})", schema.name, schema.type_tag);
        println!("  └─ id: {}...", &schema.id[..16]);
    }

    Ok(())
}

// ── Messages ──────────────────────────────────────────────────────────────────

#[derive(Deserialize)]
struct MessagesResponse {
    peer_pubkey: String,
    messages: Vec<MessageJson>,
}

#[derive(Deserialize)]
#[allow(dead_code)]
struct MessageJson {
    msg_id: String,
    from: String,
    to: String,
    msg_type: String,
    timestamp: u64,
    content: serde_json::Value,
}

#[derive(Serialize)]
struct SendMessageRequest {
    to: String,
    text: String,
}

#[derive(Deserialize)]
struct SendMessageResponse {
    msg_id: String,
    timestamp: u64,
}

async fn cmd_messages(port: u16, peer_pubkey: &str) -> Result<()> {
    let resp: MessagesResponse =
        get_json(&format!("{}/messages/{}", base_url(port), peer_pubkey)).await?;

    if resp.messages.is_empty() {
        println!(
            "No messages from {}...",
            &resp.peer_pubkey[..16.min(resp.peer_pubkey.len())]
        );
        return Ok(());
    }

    println!("═══════════════════════════════════════");
    println!(
        "  Messages from {}...",
        &resp.peer_pubkey[..16.min(resp.peer_pubkey.len())]
    );
    println!("═══════════════════════════════════════");

    for m in &resp.messages {
        println!("  ┌─ {} [{}]", m.msg_type, m.timestamp);
        println!("  │  from : {}...", &m.from[..16.min(m.from.len())]);
        println!("  │  id   : {}...", &m.msg_id[..16.min(m.msg_id.len())]);
        if let Some(text) = m.content.get("text").and_then(|v| v.as_str()) {
            println!("  └─ {}", text);
        } else {
            println!("  └─ {:?}", m.content);
        }
    }

    Ok(())
}

async fn cmd_messages_send(port: u16, to: &str, text: &str) -> Result<()> {
    let req = SendMessageRequest {
        to: to.to_string(),
        text: text.to_string(),
    };

    let resp: SendMessageResponse =
        post_json_body(&format!("{}/messages/send", base_url(port)), &req).await?;

    println!("Message sent:");
    println!(
        "  ID        : {}...",
        &resp.msg_id[..16.min(resp.msg_id.len())]
    );
    println!("  Timestamp : {}", resp.timestamp);

    Ok(())
}

async fn cmd_services(port: u16) -> Result<()> {
    let resp: ServicesResponse = get_json(&format!("{}/services", base_url(port))).await?;

    println!("═══════════════════════════════════════");
    println!("  Services");
    println!("═══════════════════════════════════════");

    for svc in &resp.services {
        let icon = if svc.enabled { "✓" } else { "○" };
        let state = if svc.enabled { "enabled" } else { "disabled" };
        println!("  {} {:<16} {} ({})", icon, svc.name, state, svc.contract);
    }

    Ok(())
}

async fn cmd_compute_tasks(port: u16, peer_pubkey: &str) -> Result<()> {
    let resp: ComputeTasksResponse =
        get_json(&format!("{}/compute/tasks/{}", base_url(port), peer_pubkey)).await?;

    if resp.tasks.is_empty() {
        println!(
            "No compute tasks from {}...",
            &resp.peer_pubkey[..16.min(resp.peer_pubkey.len())]
        );
        return Ok(());
    }

    println!("═══════════════════════════════════════");
    println!(
        "  Compute Tasks from {}...",
        &resp.peer_pubkey[..16.min(resp.peer_pubkey.len())]
    );
    println!("═══════════════════════════════════════");

    for t in &resp.tasks {
        println!("  ┌─ {}...", &t.task_id[..16.min(t.task_id.len())]);
        println!("  │  status       : {}", t.status);
        println!("  │  submitted_at : {}", t.submitted_at);
        println!("  │  updated_at   : {}", t.updated_at);
        if let Some(ms) = t.elapsed_ms {
            println!("  │  elapsed      : {}ms", ms);
        }
        if let Some(ref result) = t.result {
            print_result(result);
        }
        println!("  └─");
    }

    Ok(())
}

#[derive(Deserialize)]
struct ComputeAllTasksResponse {
    tasks: Vec<ComputeTaskJson>,
}

async fn cmd_compute_tasks_all(port: u16) -> Result<()> {
    let resp: ComputeAllTasksResponse =
        get_json(&format!("{}/compute/tasks", base_url(port))).await?;

    if resp.tasks.is_empty() {
        println!("No compute tasks.");
        return Ok(());
    }

    println!("═══════════════════════════════════════");
    println!("  All Compute Tasks ({})", resp.tasks.len());
    println!("═══════════════════════════════════════");

    for t in &resp.tasks {
        println!("  ┌─ {}...", &t.task_id[..16.min(t.task_id.len())]);
        println!("  │  status       : {}", t.status);
        println!("  │  submitted_at : {}", t.submitted_at);
        println!("  │  updated_at   : {}", t.updated_at);
        if let Some(ms) = t.elapsed_ms {
            println!("  │  elapsed      : {}ms", ms);
        }
        if let Some(ref result) = t.result {
            print_result(result);
        }
        println!("  └─");
    }

    Ok(())
}

fn print_result(result: &serde_json::Value) {
    if let Some(stdout) = result.get("stdout").and_then(|v| v.as_str()) {
        if !stdout.is_empty() {
            println!("  │  stdout       : {}", stdout.trim());
        }
    }
    if let Some(stderr) = result.get("stderr").and_then(|v| v.as_str()) {
        if !stderr.is_empty() {
            println!("  │  stderr       : {}", stderr.trim());
        }
    }
    if let Some(error) = result.get("error").and_then(|v| v.as_str()) {
        println!("  │  error        : {}", error.trim());
    }
    if let Some(files) = result.get("output_files").and_then(|v| v.as_array()) {
        let names: Vec<&str> = files.iter().filter_map(|v| v.as_str()).collect();
        if !names.is_empty() {
            println!("  │  output files : {}", names.join(", "));
        }
    }
}

async fn cmd_compute_submit(port: u16, to: &str, payload_str: &str) -> Result<()> {
    let payload: serde_json::Value =
        serde_json::from_str(payload_str).context("payload must be valid JSON")?;

    let req = ComputeSubmitRequest {
        to: to.to_string(),
        payload,
    };

    let resp: ComputeSubmitResponse =
        post_json_body(&format!("{}/compute/submit", base_url(port)), &req).await?;

    println!("Compute task submitted:");
    println!(
        "  Task ID   : {}...",
        &resp.task_id[..16.min(resp.task_id.len())]
    );
    println!("  Timestamp : {}", resp.timestamp);

    Ok(())
}

fn print_usage() {
    println!("Usage: summit-ctl [--port <port>] <command>");
    println!();
    println!("Daemon");
    println!("  shutdown                        Gracefully shut down the daemon");
    println!("  status                          Sessions, cache, and peer summary");
    println!("  services                        Show enabled/disabled services");
    println!();
    println!("Peers & Sessions");
    println!("  peers                           List discovered peers with trust status");
    println!("  sessions drop <id>              Drop a specific session");
    println!("  sessions inspect <id>           Show detailed session info");
    println!();
    println!("Trust");
    println!("  trust list                      Show trust rules");
    println!("  trust add <pubkey>              Trust a peer (flushes buffered chunks)");
    println!("  trust block <pubkey>            Block a peer");
    println!("  trust pending                   Untrusted peers with buffered chunks");
    println!();
    println!("File Transfer");
    println!("  send <file>                     Broadcast file to all trusted peers");
    println!("  send <file> --peer <pubkey>     Send file to specific peer");
    println!("  send <file> --session <id>      Send file to specific session");
    println!("  files                           List received and in-progress files");
    println!();
    println!("Messaging");
    println!("  messages <pubkey>               List messages from a peer");
    println!("  messages send <pubkey> <text>   Send a text message to a peer");
    println!();
    println!("Compute");
    println!("  compute tasks                   List all compute tasks");
    println!("  compute tasks <pubkey>          List compute tasks from a specific peer");
    println!("  compute submit <pubkey> -- <cmd>  Submit a shell command to a peer");
    println!("  compute submit <pubkey> <json>    Submit a JSON task payload");
    println!();
    println!("Cache & Schema");
    println!("  cache                           Show cache statistics");
    println!("  cache clear                     Clear the chunk cache");
    println!("  schema list                     List all known schemas");
    println!();
    println!("Options:");
    println!(
        "  --port <port>                   API port (default: {})",
        DEFAULT_PORT
    );
    println!();
    println!("Examples:");
    println!("  summit-ctl status");
    println!("  summit-ctl services");
    println!("  summit-ctl trust add 5c8c7d3c9eff6572...");
    println!("  summit-ctl send document.pdf");
    println!("  summit-ctl send photo.jpg --peer 99b1db0b1849c7f8...");
    println!("  summit-ctl messages send 99b1db0b... 'hello world'");
    println!("  summit-ctl compute submit 99b1db0b... -- uname -a");
    println!("  summit-ctl compute submit 99b1db0b... -- hostnamectl > info.txt");
    println!("  summit-ctl compute tasks");
}

// ── Entry point ───────────────────────────────────────────────────────────────

#[tokio::main]
async fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();

    // Parse --port option
    let mut port = DEFAULT_PORT;
    let mut remaining: Vec<String> = Vec::new();
    let mut i = 0;
    while i < args.len() {
        if args[i] == "--port" {
            i += 1;
            port = args
                .get(i)
                .context("--port requires a value")?
                .parse()
                .context("--port must be a number")?;
        } else {
            remaining.push(args[i].clone());
        }
        i += 1;
    }

    // Convert to string slices for matching
    let remaining_refs: Vec<&str> = remaining.iter().map(|s| s.as_str()).collect();

    // Handle send command with optional targeting
    if remaining_refs.first() == Some(&"send") && remaining_refs.len() >= 2 {
        let path = remaining_refs[1];
        let mut target_peer = None;
        let mut target_session = None;

        let mut i = 2;
        while i < remaining_refs.len() {
            match remaining_refs[i] {
                "--peer" => {
                    i += 1;
                    target_peer = remaining_refs.get(i).copied(); // Add .copied()
                }
                "--session" => {
                    i += 1;
                    target_session = remaining_refs.get(i).copied(); // Add .copied()
                }
                _ => {
                    anyhow::bail!("Unknown option: {}", remaining_refs[i]);
                }
            }
            i += 1;
        }

        return cmd_send(port, path, target_peer, target_session).await;
    }

    // Handle: compute submit <pubkey> -- <shell command...>
    // Everything after "--" is joined into a single shell string.
    if remaining_refs.len() >= 4 && remaining_refs[0] == "compute" && remaining_refs[1] == "submit"
    {
        if let Some(sep) = remaining_refs.iter().position(|s| *s == "--") {
            let to = remaining_refs[2];
            let shell_cmd = remaining[sep + 1..].join(" ");
            let payload = serde_json::json!({ "run": shell_cmd }).to_string();
            return cmd_compute_submit(port, to, &payload).await;
        }
    }

    match remaining_refs.as_slice() {
        ["shutdown"] => cmd_shutdown(port).await,
        ["status"] | [] => cmd_status(port).await,
        ["services"] => cmd_services(port).await,
        ["peers"] => cmd_peers(port).await,
        ["sessions", "drop", id] => cmd_session_drop(port, id).await,
        ["sessions", "inspect", id] => cmd_session_inspect(port, id).await,
        ["cache"] => cmd_cache(port).await,
        ["cache", "clear"] => cmd_cache_clear(port).await,
        ["files"] => cmd_files(port).await,
        ["trust", "list"] | ["trust"] => cmd_trust_list(port).await,
        ["trust", "add", pubkey] => cmd_trust_add(port, pubkey).await,
        ["trust", "block", pubkey] => cmd_trust_block(port, pubkey).await,
        ["trust", "pending"] => cmd_trust_pending(port).await,
        ["messages", peer] => cmd_messages(port, peer).await,
        ["messages", "send", to, text] => cmd_messages_send(port, to, text).await,
        ["compute", "tasks"] => cmd_compute_tasks_all(port).await,
        ["compute", "tasks", peer] => cmd_compute_tasks(port, peer).await,
        ["compute", "submit", to, payload] => cmd_compute_submit(port, to, payload).await,
        ["schema", "list"] | ["schema"] => cmd_schema_list(port).await,
        ["help"] | ["--help"] | ["-h"] => {
            print_usage();
            Ok(())
        }
        other => {
            eprintln!("Unknown command: {}", other.join(" "));
            eprintln!();
            print_usage();
            std::process::exit(1);
        }
    }
}
