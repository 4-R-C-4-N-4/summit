//! File transfer — chunking, reassembly, and metadata.

use anyhow::{Context, Result};
use bytes::Bytes;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Instant;
use tokio::sync::Mutex;

use crate::chunk_types::OutgoingChunk;
use crate::schema::KnownSchema;

/// Maximum chunk payload size (before encryption overhead)
pub const MAX_CHUNK_SIZE: usize = 32 * 1024; // 32KB

/// File metadata — sent as the first chunk of a file transfer
#[derive(serde::Serialize, serde::Deserialize, Clone)]
pub struct FileMetadata {
    pub filename: String,
    pub total_bytes: u64,
    pub chunk_hashes: Vec<[u8; 32]>,
}

/// Chunk a file into multiple OutgoingChunks
pub fn chunk_file(path: &std::path::Path) -> Result<Vec<OutgoingChunk>> {
    let data =
        std::fs::read(path).with_context(|| format!("failed to read file: {}", path.display()))?;

    let filename = path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("unknown")
        .to_string();

    let mut chunks = Vec::new();
    let mut chunk_hashes = Vec::new();

    // Split file into data chunks
    for chunk_data in data.chunks(MAX_CHUNK_SIZE) {
        let content_hash = summit_core::crypto::hash(chunk_data);
        chunk_hashes.push(content_hash);

        chunks.push(OutgoingChunk {
            type_tag: 2, // File data chunk
            schema_id: KnownSchema::FileData.id(),
            payload: Bytes::copy_from_slice(chunk_data),
            priority_flags: 0x02, // Bulk
        });
    }

    // Create metadata chunk (goes first)
    let metadata = FileMetadata {
        filename,
        total_bytes: data.len() as u64,
        chunk_hashes: chunk_hashes.clone(),
    };

    let metadata_bytes = serde_json::to_vec(&metadata)?;
    chunks.insert(
        0,
        OutgoingChunk {
            type_tag: 3, // File metadata
            schema_id: KnownSchema::FileMetadata.id(),
            payload: Bytes::from(metadata_bytes),
            priority_flags: 0x02, // Bulk
        },
    );

    Ok(chunks)
}

/// Tracks files being reassembled from incoming chunks
pub struct FileReassembler {
    /// In-progress file reassembly state
    active: Arc<Mutex<HashMap<String, FileAssembly>>>,
    /// Where to write completed files
    output_dir: PathBuf,
}

struct FileAssembly {
    metadata: FileMetadata,
    chunks_received: HashMap<[u8; 32], Bytes>,
    started_at: Instant,
    last_chunk_at: Instant,
    nack_count: u8,
    sender_pubkey: [u8; 32],
    missing_at_last_nack: usize,
}

/// Info about a stalled file assembly, returned by `stalled_assemblies()`.
pub struct StalledAssembly {
    pub filename: String,
    pub missing: Vec<[u8; 32]>,
    pub attempt: u8,
    pub sender_pubkey: [u8; 32],
}

/// Maximum age for an in-progress file assembly before it is considered stale.
const ASSEMBLY_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(300);

impl FileReassembler {
    pub fn new(output_dir: PathBuf) -> Self {
        std::fs::create_dir_all(&output_dir).ok();
        Self {
            active: Arc::new(Mutex::new(HashMap::new())),
            output_dir,
        }
    }

    /// Process a metadata chunk — start tracking this file.
    ///
    /// `sender_pubkey` is the peer that sent the metadata, used for targeted
    /// NACK recovery (attempt 0 goes to the original sender).
    pub async fn add_metadata(&self, metadata: FileMetadata, sender_pubkey: [u8; 32]) {
        let mut active = self.active.lock().await;
        Self::cleanup_stale(&mut active);
        let now = Instant::now();
        active.insert(
            metadata.filename.clone(),
            FileAssembly {
                metadata,
                chunks_received: HashMap::new(),
                started_at: now,
                last_chunk_at: now,
                nack_count: 0,
                sender_pubkey,
                missing_at_last_nack: 0,
            },
        );
    }

    /// Remove assemblies older than `ASSEMBLY_TIMEOUT`.
    fn cleanup_stale(active: &mut HashMap<String, FileAssembly>) {
        active.retain(|filename, assembly| {
            let stale = assembly.started_at.elapsed() > ASSEMBLY_TIMEOUT;
            if stale {
                tracing::warn!(filename, "removing stale file assembly (timed out)");
            }
            !stale
        });
    }

    /// Process a data chunk — add to file assembly
    pub async fn add_chunk(&self, content_hash: [u8; 32], data: Bytes) -> Result<Option<PathBuf>> {
        let mut active = self.active.lock().await;
        Self::cleanup_stale(&mut active);
        // Find which file this chunk belongs to
        let mut completed_file: Option<(String, PathBuf)> = None;

        for (filename, assembly) in active.iter_mut() {
            if assembly.metadata.chunk_hashes.contains(&content_hash) {
                assembly.chunks_received.insert(content_hash, data);
                assembly.last_chunk_at = Instant::now();

                // Check if complete
                if assembly.chunks_received.len() == assembly.metadata.chunk_hashes.len() {
                    // Reassemble
                    let mut file_data = Vec::new();
                    for hash in &assembly.metadata.chunk_hashes {
                        if let Some(chunk) = assembly.chunks_received.get(hash) {
                            file_data.extend_from_slice(chunk);
                        }
                    }

                    let output_path = self.output_dir.join(&assembly.metadata.filename);
                    std::fs::write(&output_path, file_data)?;

                    #[cfg(unix)]
                    {
                        use std::os::unix::fs::PermissionsExt;
                        std::fs::set_permissions(
                            &output_path,
                            std::fs::Permissions::from_mode(0o755),
                        )?;
                    }

                    tracing::info!(
                        filename = %assembly.metadata.filename,
                        bytes = assembly.metadata.total_bytes,
                        chunks = assembly.metadata.chunk_hashes.len(),
                                   path = %output_path.display(),
                                   "file received and reassembled"
                    );

                    completed_file = Some((filename.clone(), output_path));
                    break;
                }
                break;
            }
        }

        // Remove from active after iteration complete
        if let Some((filename, path)) = completed_file {
            active.remove(&filename);
            return Ok(Some(path));
        }
        Ok(None)
    }

    /// Clone the inner state (for use in sync-to-async bridges).
    fn clone_inner(&self) -> FileReassembler {
        FileReassembler {
            active: self.active.clone(),
            output_dir: self.output_dir.clone(),
        }
    }

    /// List files currently being received
    pub async fn in_progress(&self) -> Vec<String> {
        self.active.lock().await.keys().cloned().collect()
    }

    /// For each in-progress assembly, return the list of content hashes
    /// that have not yet been received.
    pub async fn missing_chunks(&self) -> Vec<(String, Vec<[u8; 32]>)> {
        let active = self.active.lock().await;
        active
            .iter()
            .map(|(filename, assembly)| {
                let missing: Vec<[u8; 32]> = assembly
                    .metadata
                    .chunk_hashes
                    .iter()
                    .filter(|h| !assembly.chunks_received.contains_key(*h))
                    .copied()
                    .collect();
                (filename.clone(), missing)
            })
            .filter(|(_, missing)| !missing.is_empty())
            .collect()
    }

    /// Assemblies that have stalled (no chunk received recently) and haven't
    /// exhausted their NACK attempts.
    pub async fn stalled_assemblies(
        &self,
        nack_delay: std::time::Duration,
    ) -> Vec<StalledAssembly> {
        let mut active = self.active.lock().await;
        active
            .iter_mut()
            .filter(|(_, a)| a.last_chunk_at.elapsed() > nack_delay)
            .filter_map(|(filename, a)| {
                let missing: Vec<[u8; 32]> = a
                    .metadata
                    .chunk_hashes
                    .iter()
                    .filter(|h| !a.chunks_received.contains_key(*h))
                    .copied()
                    .collect();
                if missing.is_empty() {
                    return None;
                }

                // Progress-aware: if chunks were recovered since last NACK, reset stall counter
                let current_missing = missing.len();
                if a.missing_at_last_nack > 0 && current_missing < a.missing_at_last_nack {
                    tracing::debug!(
                        filename,
                        was = a.missing_at_last_nack,
                        now = current_missing,
                        "progress detected, resetting nack stall counter"
                    );
                    a.nack_count = 0;
                }

                if a.nack_count >= summit_core::recovery::MAX_NACK_STALLS {
                    return None;
                }

                Some(StalledAssembly {
                    filename: filename.clone(),
                    missing,
                    attempt: a.nack_count,
                    sender_pubkey: a.sender_pubkey,
                })
            })
            .collect()
    }

    /// Mark that a NACK was sent for this assembly, recording how many
    /// chunks were missing at this point for progress detection.
    pub async fn increment_nack_count(&self, filename: &str, missing_count: usize) {
        let mut active = self.active.lock().await;
        if let Some(assembly) = active.get_mut(filename) {
            assembly.nack_count += 1;
            assembly.missing_at_last_nack = missing_count;
        }
    }

    /// Remove an assembly permanently. Called when recovery is impossible.
    pub async fn abandon(&self, filename: &str) {
        let mut active = self.active.lock().await;
        if active.remove(filename).is_some() {
            tracing::warn!(filename, "file assembly abandoned — chunks unrecoverable");
        }
    }
}

// ── ChunkService implementation ───────────────────────────────────────────────

use crate::service::ChunkService;
use summit_core::wire::{service_hash, ChunkHeader, Contract, ServiceHash};

impl ChunkService for FileReassembler {
    fn service_hash(&self) -> ServiceHash {
        service_hash(b"summit.file_transfer")
    }

    fn contract(&self) -> Contract {
        Contract::Bulk
    }

    fn on_activate(&self, peer_pubkey: &[u8; 32]) {
        tracing::info!(
            peer = hex::encode(&peer_pubkey[..8]),
            "file transfer activated"
        );
    }

    fn on_deactivate(&self, peer_pubkey: &[u8; 32]) {
        tracing::info!(
            peer = hex::encode(&peer_pubkey[..8]),
            "file transfer deactivated"
        );
    }

    fn handle_chunk(
        &self,
        peer_pubkey: &[u8; 32],
        header: &ChunkHeader,
        payload: &[u8],
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let data = Bytes::copy_from_slice(payload);
        let content_hash = header.content_hash;
        let type_tag = header.type_tag;
        let sender = *peer_pubkey;
        let this = self.clone_inner();

        tokio::task::block_in_place(|| {
            tokio::runtime::Handle::current().block_on(async move {
                if type_tag == 3 {
                    if let Ok(metadata) = serde_json::from_slice::<FileMetadata>(&data) {
                        this.add_metadata(metadata, sender).await;
                    }
                } else if type_tag == 2 {
                    if let Err(e) = this.add_chunk(content_hash, data).await {
                        tracing::warn!(error = %e, "chunk reassembly failed");
                    }
                }
            })
        });

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chunk_file_produces_metadata_and_data_chunks() {
        let dir = std::env::temp_dir().join(format!("summit-ft-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let file_path = dir.join("test.txt");
        std::fs::write(&file_path, b"hello world").unwrap();

        let chunks = chunk_file(&file_path).unwrap();
        // 1 metadata chunk + 1 data chunk (file < MAX_CHUNK_SIZE)
        assert_eq!(chunks.len(), 2);
        // First chunk is metadata (type_tag 3)
        assert_eq!(chunks[0].type_tag, 3);
        // Second chunk is data (type_tag 2)
        assert_eq!(chunks[1].type_tag, 2);

        // Metadata deserializes correctly
        let meta: FileMetadata = serde_json::from_slice(&chunks[0].payload).unwrap();
        assert_eq!(meta.filename, "test.txt");
        assert_eq!(meta.total_bytes, 11);
        assert_eq!(meta.chunk_hashes.len(), 1);

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn reassembler_completes_file() {
        let dir = std::env::temp_dir().join(format!("summit-reasm-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);

        let reassembler = FileReassembler::new(dir.clone());

        let data = b"reassembly test data";
        let hash = summit_core::crypto::hash(data);

        let metadata = FileMetadata {
            filename: "out.txt".into(),
            total_bytes: data.len() as u64,
            chunk_hashes: vec![hash],
        };

        reassembler.add_metadata(metadata, [0xAA; 32]).await;
        assert_eq!(reassembler.in_progress().await.len(), 1);

        let result = reassembler
            .add_chunk(hash, Bytes::from_static(data))
            .await
            .unwrap();
        assert!(result.is_some());

        let output_path = result.unwrap();
        assert_eq!(std::fs::read(&output_path).unwrap(), data);

        // After completion, no longer in progress
        assert!(reassembler.in_progress().await.is_empty());

        let _ = std::fs::remove_dir_all(&dir);
    }
}
