//! Session management commands.

use anyhow::{Context, Result};
use serde::Deserialize;

use super::http::{base_url, get_json};

pub async fn cmd_session_drop(port: u16, session_id: &str) -> Result<()> {
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

pub async fn cmd_session_inspect(port: u16, session_id: &str) -> Result<()> {
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
