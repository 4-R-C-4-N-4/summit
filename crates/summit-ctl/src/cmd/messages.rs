//! Messaging commands.

use anyhow::Result;
use serde::{Deserialize, Serialize};

use super::http::{base_url, get_json, post_json_body};

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

pub async fn cmd_messages(port: u16, peer_pubkey: &str) -> Result<()> {
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

pub async fn cmd_messages_send(port: u16, to: &str, text: &str) -> Result<()> {
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
