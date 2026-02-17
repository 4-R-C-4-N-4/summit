//! summit-ctl — command-line interface for the Summit daemon.

use anyhow::{Context, Result};
use serde::Deserialize;

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
struct CacheInfo {
    chunks: usize,
    bytes:  u64,
}

#[tokio::main]
async fn main() -> Result<()> {
    let port = 9001u16;
    let url  = format!("http://127.0.0.1:{}/status", port);

    let resp = reqwest::get(&url)
    .await
    .context("failed to connect to summitd — is it running?")?
    .json::<StatusResponse>()
    .await
    .context("failed to parse status response")?;

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
