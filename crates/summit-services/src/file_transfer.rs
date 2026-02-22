//! File transfer — chunking, reassembly, and metadata.

use anyhow::{Context, Result};
use bytes::Bytes;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
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
}

impl FileReassembler {
    pub fn new(output_dir: PathBuf) -> Self {
        std::fs::create_dir_all(&output_dir).ok();
        Self {
            active: Arc::new(Mutex::new(HashMap::new())),
            output_dir,
        }
    }

    /// Process a metadata chunk — start tracking this file
    pub async fn add_metadata(&self, metadata: FileMetadata) {
        let mut active = self.active.lock().await;
        active.insert(
            metadata.filename.clone(),
            FileAssembly {
                metadata,
                chunks_received: HashMap::new(),
            },
        );
    }

    /// Process a data chunk — add to file assembly
    pub async fn add_chunk(&self, content_hash: [u8; 32], data: Bytes) -> Result<Option<PathBuf>> {
        let mut active = self.active.lock().await;
        // Find which file this chunk belongs to
        let mut completed_file: Option<(String, PathBuf)> = None;

        for (filename, assembly) in active.iter_mut() {
            if assembly.metadata.chunk_hashes.contains(&content_hash) {
                assembly.chunks_received.insert(content_hash, data);

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

    /// List files currently being received
    pub async fn in_progress(&self) -> Vec<String> {
        self.active.lock().await.keys().cloned().collect()
    }
}

// ── ChunkService implementation ───────────────────────────────────────────────

use crate::service::ChunkService;
use summit_core::wire::{ChunkHeader, Contract, ServiceHash, service_hash};

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
        _peer_pubkey: &[u8; 32],
        header: &ChunkHeader,
        payload: &[u8],
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let data = Bytes::copy_from_slice(payload);
        let active = self.active.clone();
        let output_dir = self.output_dir.clone();
        let content_hash = header.content_hash;
        let type_tag = header.type_tag;

        // Use tokio::task::block_in_place to call async methods from sync context
        tokio::task::block_in_place(|| {
            tokio::runtime::Handle::current().block_on(async move {
                if type_tag == 3 {
                    // File metadata chunk
                    if let Ok(metadata) = serde_json::from_slice::<FileMetadata>(&data) {
                        let mut active = active.lock().await;
                        active.insert(
                            metadata.filename.clone(),
                            FileAssembly {
                                metadata,
                                chunks_received: HashMap::new(),
                            },
                        );
                    }
                } else if type_tag == 2 {
                    // File data chunk — reassemble
                    let mut active_lock = active.lock().await;
                    let mut completed_file: Option<(String, std::path::PathBuf)> = None;

                    for (filename, assembly) in active_lock.iter_mut() {
                        if assembly.metadata.chunk_hashes.contains(&content_hash) {
                            assembly.chunks_received.insert(content_hash, data.clone());

                            if assembly.chunks_received.len() == assembly.metadata.chunk_hashes.len() {
                                let mut file_data = Vec::new();
                                for hash in &assembly.metadata.chunk_hashes {
                                    if let Some(chunk) = assembly.chunks_received.get(hash) {
                                        file_data.extend_from_slice(chunk);
                                    }
                                }
                                let output_path = output_dir.join(&assembly.metadata.filename);
                                if let Err(e) = std::fs::write(&output_path, &file_data) {
                                    tracing::warn!(error = %e, "failed to write reassembled file");
                                } else {
                                    tracing::info!(
                                        filename = %assembly.metadata.filename,
                                        path = %output_path.display(),
                                        "file received and reassembled"
                                    );
                                }
                                completed_file = Some((filename.clone(), output_path));
                            }
                            break;
                        }
                    }

                    if let Some((filename, _)) = completed_file {
                        active_lock.remove(&filename);
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

        reassembler.add_metadata(metadata).await;
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
