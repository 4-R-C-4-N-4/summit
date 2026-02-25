//! Compute task commands.

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

use super::http::{base_url, get_json, post_json_body};

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

#[derive(Deserialize)]
struct ComputeAllTasksResponse {
    tasks: Vec<ComputeTaskJson>,
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

pub async fn cmd_compute_tasks(port: u16, peer_pubkey: &str) -> Result<()> {
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
        print_task(t);
    }

    Ok(())
}

pub async fn cmd_compute_tasks_all(port: u16) -> Result<()> {
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
        print_task(t);
    }

    Ok(())
}

pub async fn cmd_compute_submit(port: u16, to: &str, payload_str: &str) -> Result<()> {
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

fn print_task(t: &ComputeTaskJson) {
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

fn print_result(result: &serde_json::Value) {
    if let Some(stdout) = result.get("stdout").and_then(|v| v.as_str())
        && !stdout.is_empty()
    {
        println!("  │  stdout       : {}", stdout.trim());
    }
    if let Some(stderr) = result.get("stderr").and_then(|v| v.as_str())
        && !stderr.is_empty()
    {
        println!("  │  stderr       : {}", stderr.trim());
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
