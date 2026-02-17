//! summit-ctl — command-line interface for the Summit daemon.

use anyhow::{Context, Result};
use serde::Deserialize;

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
    contract:         String,
    chunk_port:       u16,
    established_secs: u64,
}

#[derive(Deserialize)]
struct PeersResponse {
    peers: Vec<PeerInfo>,
}

#[derive(Deserialize)]
struct PeerInfo {
    public_key:     String,
        addr:           String,
        session_port:   u16,
        chunk_port:     u16,
        contract:       u8,
        version:        u32,
        last_seen_secs: u64,
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
            println!("  ┌─ {}", &s.session_id[..16]);
            println!("  │  peer     : {}", s.peer);
            println!("  │  contract : {}", s.contract);
            println!("  │  port     : {}", s.chunk_port);
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
        println!("  ┌─ {}", &p.public_key[..16]);
        println!("  │  addr         : {}", p.addr);
        println!("  │  session port : {}", p.session_port);
        println!("  │  chunk port   : {}", p.chunk_port);
        println!("  │  contract     : {}", contract_name);
        println!("  │  version      : {}", p.version);
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

fn print_usage() {
    println!("Usage: summit-ctl [--port <port>] <command>");
    println!();
    println!("Commands:");
    println!("  status        Show daemon status, sessions, and cache stats");
    println!("  peers         List discovered peers");
    println!("  cache         Show cache statistics");
    println!("  cache clear   Clear the chunk cache");
    println!();
    println!("Options:");
    println!("  --port <port>   Status endpoint port (default: {})", DEFAULT_PORT);
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
        ["status"] | []                    => cmd_status(port).await,
        ["peers"]                          => cmd_peers(port).await,
        ["cache"]                          => cmd_cache(port).await,
        ["cache", "clear"]                 => cmd_cache_clear(port).await,
        ["help"] | ["--help"] | ["-h"]     => { print_usage(); Ok(()) }
        other => {
            eprintln!("Unknown command: {}", other.join(" "));
            eprintln!();
            print_usage();
            std::process::exit(1);
        }
    }
}
