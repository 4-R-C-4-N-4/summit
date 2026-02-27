//! /send, /files handlers — file transfer endpoints.

use axum::extract::{Multipart, State};
use axum::http::StatusCode;
use axum::Json;
use serde::Serialize;

use summit_services::SendTarget;

use super::ApiState;

/// Maximum upload size per file (256 MB).
const MAX_UPLOAD_BYTES: usize = 256 * 1024 * 1024;

// ── /send ─────────────────────────────────────────────────────────────────────

#[derive(Serialize)]
pub struct SendResponse {
    pub filename: String,
    pub bytes: u64,
    pub chunks_sent: usize,
}

pub async fn handle_send(
    State(state): State<ApiState>,
    mut multipart: Multipart,
) -> Result<Json<SendResponse>, (StatusCode, String)> {
    let mut file_data = Vec::new();
    let mut filename = String::from("uploaded_file");
    let mut target = SendTarget::Broadcast;

    while let Some(field) = multipart
        .next_field()
        .await
        .map_err(|e| (StatusCode::BAD_REQUEST, e.to_string()))?
    {
        let field_name = field.name().unwrap_or("").to_string();

        if field_name == "target" {
            let target_str = field
                .text()
                .await
                .map_err(|e| (StatusCode::BAD_REQUEST, e.to_string()))?;
            target = serde_json::from_str(&target_str)
                .map_err(|e| (StatusCode::BAD_REQUEST, format!("invalid target: {e}")))?;
        } else {
            if let Some(name) = field.file_name() {
                filename = sanitize_filename(name);
            }
            let data = field
                .bytes()
                .await
                .map_err(|e| (StatusCode::BAD_REQUEST, e.to_string()))?;
            if file_data.len() + data.len() > MAX_UPLOAD_BYTES {
                return Err((
                    StatusCode::PAYLOAD_TOO_LARGE,
                    format!("file exceeds {} byte limit", MAX_UPLOAD_BYTES),
                ));
            }
            file_data.extend_from_slice(&data);
        }
    }

    if file_data.is_empty() {
        return Err((StatusCode::BAD_REQUEST, "no file data".to_string()));
    }

    if filename.is_empty() {
        return Err((StatusCode::BAD_REQUEST, "empty filename".to_string()));
    }

    // Write to temp file
    let temp_path = std::env::temp_dir().join(&filename);
    std::fs::write(&temp_path, &file_data)
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    // Chunk the file
    let chunks = summit_services::chunk_file(&temp_path)
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    let bytes = file_data.len() as u64;
    let chunks_sent = chunks.len();

    // Push all chunks to send queue with target, pacing to avoid overwhelming slow receivers
    for chunk in chunks {
        state
            .chunk_tx
            .send((target.clone(), chunk))
            .await
            .map_err(|_| {
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "send queue closed".to_string(),
                )
            })?;
        tokio::time::sleep(std::time::Duration::from_millis(1)).await;
    }

    tracing::info!(
        filename,
        bytes,
        chunks_sent,
        ?target,
        "file queued for sending"
    );

    Ok(Json(SendResponse {
        filename,
        bytes,
        chunks_sent,
    }))
}

/// Sanitize a filename: strip path components, reject traversal attempts.
fn sanitize_filename(raw: &str) -> String {
    // Take only the final path component (handles both / and \ separators)
    let base = raw.rsplit(['/', '\\']).next().unwrap_or(raw);

    // Remove leading dots (no hidden files / no ".." tricks)
    let trimmed = base.trim_start_matches('.');

    // Replace any remaining problematic characters
    let clean: String = trimmed
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || c == '.' || c == '-' || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect();

    if clean.is_empty() {
        "uploaded_file".to_string()
    } else {
        clean
    }
}

// ── /files ────────────────────────────────────────────────────────────────────

#[derive(Serialize)]
pub struct FilesResponse {
    pub received: Vec<String>,
    pub in_progress: Vec<String>,
}

pub async fn handle_files(State(state): State<ApiState>) -> Json<FilesResponse> {
    let received_dir = &state.file_transfer_path;
    let mut received = Vec::new();

    if let Ok(entries) = std::fs::read_dir(received_dir) {
        for entry in entries.flatten() {
            if let Some(name) = entry.file_name().to_str() {
                received.push(name.to_string());
            }
        }
    }

    let in_progress = state.reassembler.in_progress().await;

    Json(FilesResponse {
        received,
        in_progress,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sanitize_strips_path_traversal() {
        assert_eq!(sanitize_filename("../../../etc/passwd"), "passwd");
        assert_eq!(sanitize_filename("..\\..\\windows\\system32"), "system32");
        assert_eq!(sanitize_filename("/etc/passwd"), "passwd");
    }

    #[test]
    fn sanitize_strips_leading_dots() {
        assert_eq!(sanitize_filename(".hidden"), "hidden");
        assert_eq!(sanitize_filename("..sneaky"), "sneaky");
    }

    #[test]
    fn sanitize_preserves_normal_names() {
        assert_eq!(sanitize_filename("photo.jpg"), "photo.jpg");
        assert_eq!(sanitize_filename("my-doc_v2.pdf"), "my-doc_v2.pdf");
    }

    #[test]
    fn sanitize_replaces_special_chars() {
        assert_eq!(sanitize_filename("file name (1).txt"), "file_name__1_.txt");
    }

    #[test]
    fn sanitize_handles_empty() {
        assert_eq!(sanitize_filename(""), "uploaded_file");
        assert_eq!(sanitize_filename("..."), "uploaded_file");
    }
}
