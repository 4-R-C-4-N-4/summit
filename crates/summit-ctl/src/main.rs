//! summit-ctl — command-line interface for the Summit daemon.

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

const DEFAULT_PORT: u16 = 9001;

// ── Response types ────────────────────────────────────────────────────────────

#[derive(Deserialize)]
struct StatusResponse {
    sessions:         Vec<SessionInfo>,
    cache:            CacheInfo,
    peers_discovered: usize,
}

#[derive(Deserialize)]
struct SessionInfo {
    session_id:       String,
    peer:             String,
    peer_pubkey:      String,
    contract:         String,
    chunk_port:       u16,
    established_secs: u64,
    trust_level:      String,
}

#[derive(Deserialize)]
struct PeersResponse {
    peers: Vec<PeerInfo>,
}

#[derive(Deserialize)]
struct PeerInfo {
    public_key:      String,
        addr:            String,
        session_port:    u16,
        chunk_port:      u16,
        contract:        u8,
        version:         u32,
        last_seen_secs:  u64,
        trust_level:     String,
        buffered_chunks: usize,
}

#[derive(Deserialize)]
struct CacheInfo {
    chunks: usize,
    bytes:  u64,
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
        level:      String,
}

#[derive(Serialize)]
struct TrustAddRequest {
    public_key: String,
}

#[derive(Deserialize)]
struct TrustAddResponse {
    public_key:     String,
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
    public_key:      String,
        buffered_chunks: usize,
}

#[derive(Deserialize)]
struct SendResponse {
    filename:    String,
    bytes:       u64,
    chunks_sent: usize,
}

#[derive(Deserialize)]
struct FilesResponse {
    received:    Vec<String>,
    in_progress: Vec<String>,
}

// ── HTTP helpers ──────────────────────────────────────────────────────────────

fn base_url(port: u16) -> String {
    format!("http://127.0.0.1:{}", port)
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
            println!("  │  pubkey   : {}...", &s.peer_pubkey[..16]);
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
        let contract_name = match p.contract {
            0x01 => "Realtime",
            0x02 => "Bulk",
            0x03 => "Background",
            _    => "Unknown",
        };

        let trust_icon = match p.trust_level.as_str() {
            "Trusted" => "✓",
            "Blocked" => "✗",
            _ => "?",
        };

        println!("  ┌─ {} {}...", trust_icon, &p.public_key[..16]);
        println!("  │  addr         : {}", p.addr);
        println!("  │  session port : {}", p.session_port);
        println!("  │  contract     : {}", contract_name);
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
    println!("  Bytes  : {} ({:.1} KB)", resp.bytes, resp.bytes as f64 / 1024.0);

    Ok(())
}

async fn cmd_cache_clear(port: u16) -> Result<()> {
    let resp: ClearResponse = post_json(&format!("{}/cache/clear", base_url(port))).await?;
    println!("Cleared {} chunks from cache.", resp.cleared);
    Ok(())
}

async fn cmd_send(port: u16, path: &str) -> Result<()> {
    use reqwest::multipart;

    let file_data = std::fs::read(path)
    .with_context(|| format!("failed to read file: {}", path))?;

    let filename = std::path::Path::new(path)
    .file_name()
    .and_then(|n| n.to_str())
    .unwrap_or("file")
    .to_string();

    let part = multipart::Part::bytes(file_data)
    .file_name(filename.clone());

    let form = multipart::Form::new()
    .part("file", part);

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

    println!("File queued for sending:");
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
        println!("  {} {} — {}", icon, &rule.public_key[..16], rule.level);
    }

    Ok(())
}

async fn cmd_trust_add(port: u16, pubkey: &str) -> Result<()> {
    let req = TrustAddRequest {
        public_key: pubkey.to_string(),
    };

    let resp: TrustAddResponse = post_json_body(
        &format!("{}/trust/add", base_url(port)),
                                                &req
    ).await?;

    println!("✓ Peer trusted: {}...", &resp.public_key[..16]);
    if resp.flushed_chunks > 0 {
        println!("  Processed {} buffered chunks", resp.flushed_chunks);
    }

    Ok(())
}

async fn cmd_trust_block(port: u16, pubkey: &str) -> Result<()> {
    let req = TrustBlockRequest {
        public_key: pubkey.to_string(),
    };

    let resp: TrustBlockResponse = post_json_body(
        &format!("{}/trust/block", base_url(port)),
                                                  &req
    ).await?;

    println!("✗ Peer blocked: {}...", &resp.public_key[..16]);

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
        println!("  ? {}... — {} chunks buffered",
                 &peer.public_key[..16],
                 peer.buffered_chunks);
    }

    println!("\nUse 'summit-ctl trust add <pubkey>' to trust a peer and process their buffered chunks.");

    Ok(())
}

fn print_usage() {
    println!("Usage: summit-ctl [--port <port>] <command>");
    println!();
    println!("Commands:");
    println!("  status              Show daemon status, sessions, and cache");
    println!("  peers               List discovered peers with trust status");
    println!("  cache               Show cache statistics");
    println!("  cache clear         Clear the chunk cache");
    println!("  send <file>         Send a file to all trusted peers");
    println!("  files               List received files");
    println!("  trust list          Show trust rules");
    println!("  trust add <pubkey>  Trust a peer (process buffered chunks)");
    println!("  trust block <pubkey> Block a peer");
    println!("  trust pending       List untrusted peers with buffered chunks");
    println!();
    println!("Options:");
    println!("  --port <port>       Status endpoint port (default: {})", DEFAULT_PORT);
    println!();
    println!("Examples:");
    println!("  summit-ctl status");
    println!("  summit-ctl peers");
    println!("  summit-ctl trust add 5c8c7d3c9eff6572...");
    println!("  summit-ctl send document.pdf");
}

// ── Entry point ───────────────────────────────────────────────────────────────

#[tokio::main]
async fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();

    // Parse --port option
    let mut port = DEFAULT_PORT;
    let mut remaining: Vec<&str> = Vec::new();
    let mut i = 0;
    while i < args.len() {
        if args[i] == "--port" {
            i += 1;
            port = args.get(i)
            .context("--port requires a value")?
            .parse()
            .context("--port must be a number")?;
        } else {
            remaining.push(&args[i]);
        }
        i += 1;
    }

    match remaining.as_slice() {
        ["status"] | []                       => cmd_status(port).await,
        ["peers"]                             => cmd_peers(port).await,
        ["cache"]                             => cmd_cache(port).await,
        ["cache", "clear"]                    => cmd_cache_clear(port).await,
        ["send", path]                        => cmd_send(port, path).await,
        ["files"]                             => cmd_files(port).await,
        ["trust", "list"] | ["trust"]         => cmd_trust_list(port).await,
        ["trust", "add", pubkey]              => cmd_trust_add(port, pubkey).await,
        ["trust", "block", pubkey]            => cmd_trust_block(port, pubkey).await,
        ["trust", "pending"]                  => cmd_trust_pending(port).await,
        ["help"] | ["--help"] | ["-h"]        => { print_usage(); Ok(()) }
        other => {
            eprintln!("Unknown command: {}", other.join(" "));
            eprintln!();
            print_usage();
            std::process::exit(1);
        }
    }
}
