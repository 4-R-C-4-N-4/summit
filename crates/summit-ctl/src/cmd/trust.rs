//! Trust management commands.

use anyhow::Result;
use serde::{Deserialize, Serialize};

use super::http::{base_url, get_json, post_json_body};

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

pub async fn cmd_trust_list(port: u16) -> Result<()> {
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

pub async fn cmd_trust_add(port: u16, pubkey: &str) -> Result<()> {
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

pub async fn cmd_trust_block(port: u16, pubkey: &str) -> Result<()> {
    let req = TrustBlockRequest {
        public_key: pubkey.to_string(),
    };

    let resp: TrustBlockResponse =
        post_json_body(&format!("{}/trust/block", base_url(port)), &req).await?;

    println!("✗ Peer blocked: {}", &resp.public_key[..16]);

    Ok(())
}

pub async fn cmd_trust_pending(port: u16) -> Result<()> {
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
