//! Daemon status, peers, cache, services, schema, shutdown commands.

use anyhow::Result;
use serde::Deserialize;

use super::http::{base_url, get_json, post_json};

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
struct ServicesResponse {
    services: Vec<ServiceStatus>,
}

#[derive(Deserialize)]
struct ServiceStatus {
    name: String,
    enabled: bool,
    contract: String,
}

// ── Commands ──────────────────────────────────────────────────────────────────

pub async fn cmd_status(port: u16) -> Result<()> {
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

pub async fn cmd_peers(port: u16) -> Result<()> {
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

pub async fn cmd_cache(port: u16) -> Result<()> {
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

pub async fn cmd_cache_clear(port: u16) -> Result<()> {
    let resp: ClearResponse = post_json(&format!("{}/cache/clear", base_url(port))).await?;
    println!("Cleared {} chunks from cache.", resp.cleared);
    Ok(())
}

pub async fn cmd_services(port: u16) -> Result<()> {
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

pub async fn cmd_schema_list(port: u16) -> Result<()> {
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

pub async fn cmd_shutdown(port: u16) -> Result<()> {
    #[derive(Deserialize)]
    struct ShutdownResponse {
        message: String,
    }

    let resp: ShutdownResponse = post_json(&format!("{}/daemon/shutdown", base_url(port))).await?;
    println!("{}", resp.message);
    Ok(())
}
