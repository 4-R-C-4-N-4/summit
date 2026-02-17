//! File transfer — chunking, reassembly, and metadata.

use std::path::PathBuf;
use std::collections::HashMap;
use std::sync::Arc;
use bytes::Bytes;
use tokio::sync::Mutex;
use anyhow::{Context, Result};

use summit_core::crypto::hash;
use crate::chunk::OutgoingChunk;
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
    let data = std::fs::read(path)
        .with_context(|| format!("failed to read file: {}", path.display()))?;
    
    let filename = path.file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("unknown")
        .to_string();
    
    let mut chunks = Vec::new();
    let mut chunk_hashes = Vec::new();
    
    // Split file into data chunks
    for chunk_data in data.chunks(MAX_CHUNK_SIZE) {
        let content_hash = hash(chunk_data);
        chunk_hashes.push(content_hash);
        
        chunks.push(OutgoingChunk {
            type_tag: 2,  // File data chunk
            schema_id: KnownSchema::FileData.id(),
            payload: Bytes::copy_from_slice(chunk_data),
        });
    }
    
    // Create metadata chunk (goes first)
    let metadata = FileMetadata {
        filename,
        total_bytes: data.len() as u64,
        chunk_hashes: chunk_hashes.clone(),
    };
    
    let metadata_bytes = serde_json::to_vec(&metadata)?;
    chunks.insert(0, OutgoingChunk {
        type_tag: 3,  // File metadata
        schema_id: KnownSchema::FileMetadata.id(),
        payload: Bytes::from(metadata_bytes),
    });
    
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
        active.insert(metadata.filename.clone(), FileAssembly {
            metadata,
            chunks_received: HashMap::new(),
        });
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
