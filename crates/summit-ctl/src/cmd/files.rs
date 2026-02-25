//! File transfer commands.

use anyhow::{Context, Result};
use serde::Deserialize;

use super::http::{base_url, get_json};

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

pub async fn cmd_send(
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

pub async fn cmd_files(port: u16) -> Result<()> {
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
