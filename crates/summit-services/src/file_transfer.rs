//! File transfer — chunking, reassembly, metadata, and delivery confirmation.
//!
//! A file travels as one metadata chunk (filename, size, ordered list of
//! content hashes) plus one data chunk per 32 KiB block. Chunks are
//! content-addressed, so identical blocks share a hash; reassembly works on
//! the *set* of hashes and lays them out by the metadata's order.
//!
//! Reliability:
//! - Missing data chunks are recovered by the receiver (NACK, see recovery).
//! - A missing metadata chunk can only be recovered by the sender: it re-sends
//!   the metadata until the receiver answers FILE_DONE (or gives up after
//!   `MAX_METADATA_SENDS`). The receiver answers FILE_DONE for files it
//!   already completed, so a lost FILE_DONE is also repaired.
//! - On metadata arrival the receiver pulls blocks it already holds from the
//!   chunk cache (shared blocks, re-sends, metadata arriving after its data).

use anyhow::{Context, Result};
use bytes::Bytes;
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::{mpsc, Mutex};

use crate::cache::ChunkCache;
use crate::chunk_types::OutgoingChunk;
use crate::schema::KnownSchema;
use crate::send_target::SendTarget;
use summit_core::recovery::{FileDone, MAX_METADATA_SENDS};
use summit_core::wire;

/// Maximum chunk payload size (before encryption overhead)
pub const MAX_CHUNK_SIZE: usize = 32 * 1024; // 32KB

/// File metadata — sent as the first chunk of a file transfer
#[derive(serde::Serialize, serde::Deserialize, Clone)]
pub struct FileMetadata {
    pub filename: String,
    pub total_bytes: u64,
    pub chunk_hashes: Vec<[u8; 32]>,
}

/// Chunk a file into multiple OutgoingChunks (metadata first).
pub fn chunk_file(path: &std::path::Path) -> Result<Vec<OutgoingChunk>> {
    let data =
        std::fs::read(path).with_context(|| format!("failed to read file: {}", path.display()))?;

    let filename = path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("unknown")
        .to_string();

    chunk_bytes(&filename, &data)
}

/// Chunk in-memory file contents into OutgoingChunks (metadata first).
pub fn chunk_bytes(filename: &str, data: &[u8]) -> Result<Vec<OutgoingChunk>> {
    let mut chunks = Vec::new();
    let mut chunk_hashes = Vec::new();

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

    let metadata = FileMetadata {
        filename: filename.to_string(),
        total_bytes: data.len() as u64,
        chunk_hashes,
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

/// Tracks files being reassembled from incoming chunks, and files we sent
/// that the receiver has not yet confirmed.
pub struct FileReassembler {
    /// In-progress reassemblies, keyed by metadata chunk hash.
    active: Arc<Mutex<HashMap<[u8; 32], FileAssembly>>>,
    /// Recently completed transfers (metadata hash -> when), so a re-sent
    /// metadata chunk is answered with FILE_DONE instead of starting over.
    completed: Arc<Mutex<HashMap<[u8; 32], Instant>>>,
    /// Outgoing transfers awaiting FILE_DONE, keyed by metadata hash.
    outgoing: Arc<Mutex<HashMap<[u8; 32], OutgoingTransfer>>>,
    /// Where to write completed files
    output_dir: PathBuf,
    /// Chunks we already hold (lets a new assembly start part-complete).
    cache: Option<ChunkCache>,
    /// For sending FILE_DONE.
    outbound: Option<mpsc::Sender<(SendTarget, OutgoingChunk)>>,
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

impl FileAssembly {
    /// Unique hashes the metadata lists that we don't have yet.
    fn missing(&self) -> Vec<[u8; 32]> {
        let mut seen = HashSet::new();
        self.metadata
            .chunk_hashes
            .iter()
            .filter(|h| !self.chunks_received.contains_key(*h) && seen.insert(**h))
            .copied()
            .collect()
    }

    fn is_complete(&self) -> bool {
        self.metadata
            .chunk_hashes
            .iter()
            .all(|h| self.chunks_received.contains_key(h))
    }
}

struct OutgoingTransfer {
    metadata_chunk: OutgoingChunk,
    /// Peers that have not confirmed yet.
    awaiting: HashSet<[u8; 32]>,
    sends: u8,
    last_sent: Instant,
}

/// Info about a stalled file assembly, returned by `stalled_assemblies()`.
pub struct StalledAssembly {
    /// Transfer id (metadata chunk hash).
    pub id: [u8; 32],
    pub filename: String,
    pub missing: Vec<[u8; 32]>,
    pub attempt: u8,
    pub sender_pubkey: [u8; 32],
}

/// Maximum age for an in-progress file assembly before it is considered stale.
const ASSEMBLY_TIMEOUT: Duration = Duration::from_secs(300);

/// How long a completed transfer is remembered for re-acknowledgement.
const COMPLETED_MEMORY: Duration = Duration::from_secs(600);

impl FileReassembler {
    pub fn new(output_dir: PathBuf) -> Self {
        std::fs::create_dir_all(&output_dir).ok();
        Self {
            active: Arc::new(Mutex::new(HashMap::new())),
            completed: Arc::new(Mutex::new(HashMap::new())),
            outgoing: Arc::new(Mutex::new(HashMap::new())),
            output_dir,
            cache: None,
            outbound: None,
        }
    }

    /// Use the node's chunk cache to fill new assemblies with blocks we
    /// already hold.
    pub fn with_cache(mut self, cache: ChunkCache) -> Self {
        self.cache = Some(cache);
        self
    }

    /// Enable FILE_DONE confirmations to senders.
    pub fn with_outbound(mut self, tx: mpsc::Sender<(SendTarget, OutgoingChunk)>) -> Self {
        self.outbound = Some(tx);
        self
    }

    // ── Receiving ────────────────────────────────────────────────────────────

    /// Process a metadata chunk — start tracking this file.
    ///
    /// `metadata_hash` is the metadata chunk's content hash (the transfer
    /// id). Returns the output path if the file completed immediately
    /// (every block was already cached).
    pub async fn add_metadata(
        &self,
        metadata_hash: [u8; 32],
        metadata: FileMetadata,
        sender_pubkey: [u8; 32],
    ) -> Result<Option<PathBuf>> {
        {
            let mut completed = self.completed.lock().await;
            completed.retain(|_, at| at.elapsed() < COMPLETED_MEMORY);
            if completed.contains_key(&metadata_hash) {
                // Sender didn't get our FILE_DONE; repeat it.
                drop(completed);
                self.send_file_done(metadata_hash, sender_pubkey).await;
                return Ok(None);
            }
        }

        let mut metadata = metadata;
        metadata.filename = sanitize_filename(&metadata.filename);

        let mut active = self.active.lock().await;
        Self::cleanup_stale(&mut active);
        if active.contains_key(&metadata_hash) {
            return Ok(None); // re-sent metadata for a transfer in progress
        }

        let now = Instant::now();
        let mut assembly = FileAssembly {
            metadata,
            chunks_received: HashMap::new(),
            started_at: now,
            last_chunk_at: now,
            nack_count: 0,
            sender_pubkey,
            missing_at_last_nack: 0,
        };
        if let Some(cache) = &self.cache {
            for hash in assembly.missing() {
                if let Ok(Some(data)) = cache.get(&hash) {
                    assembly.chunks_received.insert(hash, data);
                }
            }
        }

        if assembly.is_complete() {
            drop(active);
            let path = self.finish(metadata_hash, &assembly).await?;
            return Ok(Some(path));
        }
        active.insert(metadata_hash, assembly);
        Ok(None)
    }

    /// Remove assemblies older than `ASSEMBLY_TIMEOUT`.
    fn cleanup_stale(active: &mut HashMap<[u8; 32], FileAssembly>) {
        active.retain(|_, assembly| {
            let stale = assembly.started_at.elapsed() > ASSEMBLY_TIMEOUT;
            if stale {
                tracing::warn!(
                    filename = assembly.metadata.filename,
                    "removing stale file assembly (timed out)"
                );
            }
            !stale
        });
    }

    /// Process a data chunk. A block can belong to several in-progress files;
    /// every one of them gets it. Returns the files that completed.
    pub async fn add_chunk(&self, content_hash: [u8; 32], data: Bytes) -> Result<Vec<PathBuf>> {
        let mut active = self.active.lock().await;
        Self::cleanup_stale(&mut active);
        let mut done = Vec::new();
        for (id, assembly) in active.iter_mut() {
            if assembly.metadata.chunk_hashes.contains(&content_hash) {
                assembly.chunks_received.insert(content_hash, data.clone());
                assembly.last_chunk_at = Instant::now();
                if assembly.is_complete() {
                    done.push(*id);
                }
            }
        }
        let finished: Vec<([u8; 32], FileAssembly)> = done
            .iter()
            .filter_map(|id| active.remove(id).map(|a| (*id, a)))
            .collect();
        drop(active);

        let mut paths = Vec::new();
        for (id, assembly) in finished {
            paths.push(self.finish(id, &assembly).await?);
        }
        Ok(paths)
    }

    /// Write a complete file, remember it, and confirm to the sender.
    async fn finish(&self, id: [u8; 32], assembly: &FileAssembly) -> Result<PathBuf> {
        let mut file_data = Vec::with_capacity(assembly.metadata.total_bytes as usize);
        for hash in &assembly.metadata.chunk_hashes {
            let chunk = assembly
                .chunks_received
                .get(hash)
                .context("assembly marked complete with a block missing")?;
            file_data.extend_from_slice(chunk);
        }

        let output_path = self.output_dir.join(&assembly.metadata.filename);
        std::fs::write(&output_path, file_data)?;

        // Never executable: this file came off the network.
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&output_path, std::fs::Permissions::from_mode(0o644))?;
        }

        tracing::info!(
            filename = %assembly.metadata.filename,
            bytes = assembly.metadata.total_bytes,
            chunks = assembly.metadata.chunk_hashes.len(),
            path = %output_path.display(),
            "file received and reassembled"
        );

        self.completed.lock().await.insert(id, Instant::now());
        self.send_file_done(id, assembly.sender_pubkey).await;
        Ok(output_path)
    }

    async fn send_file_done(&self, metadata_hash: [u8; 32], sender: [u8; 32]) {
        let Some(tx) = &self.outbound else { return };
        let Ok(payload) = serde_json::to_vec(&FileDone { metadata_hash }) else {
            return;
        };
        let chunk = OutgoingChunk {
            type_tag: wire::recovery::FILE_DONE,
            schema_id: wire::recovery_hash(),
            payload: Bytes::from(payload),
            priority_flags: 0x01, // control traffic, not rate-limited
        };
        let target = SendTarget::Peer { public_key: sender };
        if tx.send((target, chunk)).await.is_err() {
            tracing::warn!("failed to queue FILE_DONE");
        }
    }

    /// Clone the inner state (for use in sync-to-async bridges).
    fn clone_inner(&self) -> FileReassembler {
        FileReassembler {
            active: self.active.clone(),
            completed: self.completed.clone(),
            outgoing: self.outgoing.clone(),
            output_dir: self.output_dir.clone(),
            cache: self.cache.clone(),
            outbound: self.outbound.clone(),
        }
    }

    /// List files currently being received
    pub async fn in_progress(&self) -> Vec<String> {
        self.active
            .lock()
            .await
            .values()
            .map(|a| a.metadata.filename.clone())
            .collect()
    }

    /// For each in-progress assembly, the unique content hashes not yet
    /// received.
    pub async fn missing_chunks(&self) -> Vec<(String, Vec<[u8; 32]>)> {
        let active = self.active.lock().await;
        active
            .values()
            .map(|a| (a.metadata.filename.clone(), a.missing()))
            .filter(|(_, missing)| !missing.is_empty())
            .collect()
    }

    /// Assemblies that have stalled (no chunk received recently) and haven't
    /// exhausted their NACK attempts.
    pub async fn stalled_assemblies(&self, nack_delay: Duration) -> Vec<StalledAssembly> {
        let mut active = self.active.lock().await;
        active
            .iter_mut()
            .filter(|(_, a)| a.last_chunk_at.elapsed() > nack_delay)
            .filter_map(|(id, a)| {
                let missing = a.missing();
                if missing.is_empty() {
                    return None;
                }

                // Progress-aware: if chunks were recovered since last NACK, reset stall counter
                let current_missing = missing.len();
                if a.missing_at_last_nack > 0 && current_missing < a.missing_at_last_nack {
                    tracing::debug!(
                        filename = a.metadata.filename,
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
                    id: *id,
                    filename: a.metadata.filename.clone(),
                    missing,
                    attempt: a.nack_count,
                    sender_pubkey: a.sender_pubkey,
                })
            })
            .collect()
    }

    /// Mark that a NACK was sent for this assembly, recording how many
    /// chunks were missing at this point for progress detection.
    pub async fn increment_nack_count(&self, id: &[u8; 32], missing_count: usize) {
        let mut active = self.active.lock().await;
        if let Some(assembly) = active.get_mut(id) {
            assembly.nack_count = assembly.nack_count.saturating_add(1);
            assembly.missing_at_last_nack = missing_count;
        }
    }

    /// Remove an assembly permanently. Called when recovery is impossible.
    pub async fn abandon(&self, id: &[u8; 32]) {
        let mut active = self.active.lock().await;
        if let Some(a) = active.remove(id) {
            tracing::warn!(
                filename = a.metadata.filename,
                "file assembly abandoned — chunks unrecoverable"
            );
        }
    }

    // ── Sending ──────────────────────────────────────────────────────────────

    /// Track a file we just sent so its metadata is re-sent until each
    /// recipient confirms. `metadata_chunk` must be the transfer's first chunk.
    pub async fn track_outgoing(&self, metadata_chunk: OutgoingChunk, recipients: Vec<[u8; 32]>) {
        if recipients.is_empty() {
            return;
        }
        let id = summit_core::crypto::hash(&metadata_chunk.payload);
        self.outgoing.lock().await.insert(
            id,
            OutgoingTransfer {
                metadata_chunk,
                awaiting: recipients.into_iter().collect(),
                sends: 1,
                last_sent: Instant::now(),
            },
        );
    }

    /// A recipient confirmed a transfer.
    pub async fn confirm_outgoing(&self, metadata_hash: &[u8; 32], peer: &[u8; 32]) {
        let mut outgoing = self.outgoing.lock().await;
        if let Some(t) = outgoing.get_mut(metadata_hash) {
            if t.awaiting.remove(peer) {
                tracing::info!(
                    peer = hex::encode(&peer[..8]),
                    transfer = hex::encode(&metadata_hash[..8]),
                    "file delivery confirmed"
                );
            }
            if t.awaiting.is_empty() {
                outgoing.remove(metadata_hash);
            }
        }
    }

    /// Transfers whose metadata should be re-sent now, with the peers still
    /// awaiting it. Gives up (with a warning) after `MAX_METADATA_SENDS`.
    pub async fn due_metadata_resends(
        &self,
        interval: Duration,
    ) -> Vec<(OutgoingChunk, Vec<[u8; 32]>)> {
        let mut outgoing = self.outgoing.lock().await;
        let mut due = Vec::new();
        outgoing.retain(|id, t| {
            if t.last_sent.elapsed() < interval {
                return true;
            }
            if t.sends >= MAX_METADATA_SENDS {
                tracing::warn!(
                    transfer = hex::encode(&id[..8]),
                    unconfirmed = t.awaiting.len(),
                    "file delivery unconfirmed, giving up"
                );
                return false;
            }
            t.sends += 1;
            t.last_sent = Instant::now();
            due.push((
                t.metadata_chunk.clone(),
                t.awaiting.iter().copied().collect(),
            ));
            true
        });
        due
    }

    /// Outgoing transfers not yet confirmed by every recipient.
    pub async fn unconfirmed_outgoing(&self) -> usize {
        self.outgoing.lock().await.len()
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
    ) -> anyhow::Result<()> {
        let data = Bytes::copy_from_slice(payload);
        let content_hash = header.content_hash;
        let type_tag = header.type_tag;
        let sender = *peer_pubkey;
        let this = self.clone_inner();

        tokio::task::block_in_place(|| {
            tokio::runtime::Handle::current().block_on(async move {
                let result = if type_tag == 3 {
                    match serde_json::from_slice::<FileMetadata>(&data) {
                        Ok(metadata) => this
                            .add_metadata(content_hash, metadata, sender)
                            .await
                            .map(|_| ()),
                        Err(e) => Err(e.into()),
                    }
                } else if type_tag == 2 {
                    this.add_chunk(content_hash, data).await.map(|_| ())
                } else {
                    Ok(())
                };
                if let Err(e) = result {
                    tracing::warn!(error = %e, "file transfer chunk handling failed");
                }
            })
        });

        Ok(())
    }
}

/// Sanitize a filename received from a peer: strip path components, reject
/// traversal attacks. Prevents a malicious sender from writing outside the
/// output directory via `../../.bashrc` or similar.
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
        "received_file".to_string()
    } else {
        clean
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("summit-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        dir
    }

    /// Split `chunk_bytes` output into (metadata hash, metadata, data blocks).
    fn split(chunks: &[OutgoingChunk]) -> ([u8; 32], FileMetadata, Vec<Bytes>) {
        let meta_hash = summit_core::crypto::hash(&chunks[0].payload);
        let meta: FileMetadata = serde_json::from_slice(&chunks[0].payload).unwrap();
        let data = chunks[1..].iter().map(|c| c.payload.clone()).collect();
        (meta_hash, meta, data)
    }

    #[test]
    fn chunk_file_produces_metadata_and_data_chunks() {
        let dir = temp_dir("ft-test");
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
        let dir = temp_dir("reasm-test");
        let reassembler = FileReassembler::new(dir.clone());

        let data = b"reassembly test data";
        let hash = summit_core::crypto::hash(data);

        let metadata = FileMetadata {
            filename: "out.txt".into(),
            total_bytes: data.len() as u64,
            chunk_hashes: vec![hash],
        };

        let done = reassembler
            .add_metadata([1; 32], metadata, [0xAA; 32])
            .await
            .unwrap();
        assert!(done.is_none());
        assert_eq!(reassembler.in_progress().await.len(), 1);

        let paths = reassembler
            .add_chunk(hash, Bytes::from_static(data))
            .await
            .unwrap();
        assert_eq!(paths.len(), 1);
        assert_eq!(std::fs::read(&paths[0]).unwrap(), data);

        // After completion, no longer in progress
        assert!(reassembler.in_progress().await.is_empty());

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn repeated_blocks_complete() {
        // 4 identical 32 KiB blocks: one distinct hash, listed four times.
        let dir = temp_dir("reasm-repeat");
        let reassembler = FileReassembler::new(dir.clone());
        let data = vec![0u8; 4 * MAX_CHUNK_SIZE];
        let (id, meta, blocks) = split(&chunk_bytes("zeros.bin", &data).unwrap());
        assert_eq!(meta.chunk_hashes.len(), 4);

        reassembler.add_metadata(id, meta, [2; 32]).await.unwrap();
        // The receive path delivers each distinct block once.
        let paths = reassembler
            .add_chunk(summit_core::crypto::hash(&blocks[0]), blocks[0].clone())
            .await
            .unwrap();
        assert_eq!(paths.len(), 1, "file with repeated blocks must complete");
        assert_eq!(std::fs::read(&paths[0]).unwrap(), data);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn shared_block_feeds_every_file_that_needs_it() {
        let dir = temp_dir("reasm-shared");
        let reassembler = FileReassembler::new(dir.clone());
        let shared = vec![7u8; MAX_CHUNK_SIZE];
        let (id_a, meta_a, blocks_a) = split(&chunk_bytes("a.bin", &shared).unwrap());
        let (id_b, meta_b, _) = split(&chunk_bytes("b.bin", &shared).unwrap());
        assert_ne!(id_a, id_b);

        reassembler
            .add_metadata(id_a, meta_a, [2; 32])
            .await
            .unwrap();
        reassembler
            .add_metadata(id_b, meta_b, [2; 32])
            .await
            .unwrap();
        let paths = reassembler
            .add_chunk(summit_core::crypto::hash(&blocks_a[0]), blocks_a[0].clone())
            .await
            .unwrap();
        assert_eq!(paths.len(), 2, "both files need the block");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn metadata_after_data_completes_from_cache() {
        let dir = temp_dir("reasm-late-meta");
        let cache = ChunkCache::new(dir.join("cache")).unwrap();
        let reassembler = FileReassembler::new(dir.join("out")).with_cache(cache.clone());
        let data: Vec<u8> = (0..3 * MAX_CHUNK_SIZE).map(|i| (i % 251) as u8).collect();
        let (id, meta, blocks) = split(&chunk_bytes("late.bin", &data).unwrap());

        // Data arrives first (receive path caches every chunk)...
        for b in &blocks {
            cache.put(&summit_core::crypto::hash(b), b).unwrap();
            assert!(reassembler
                .add_chunk(summit_core::crypto::hash(b), b.clone())
                .await
                .unwrap()
                .is_empty());
        }
        // ...then the (re-sent) metadata finishes it immediately.
        let path = reassembler.add_metadata(id, meta, [2; 32]).await.unwrap();
        assert_eq!(std::fs::read(path.expect("completed")).unwrap(), data);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn completion_sends_file_done_and_repeats_it_for_resent_metadata() {
        let dir = temp_dir("reasm-done");
        let (tx, mut rx) = mpsc::channel(8);
        let reassembler = FileReassembler::new(dir.clone()).with_outbound(tx);
        let (id, meta, blocks) = split(&chunk_bytes("d.bin", b"done").unwrap());

        reassembler
            .add_metadata(id, meta.clone(), [9; 32])
            .await
            .unwrap();
        reassembler
            .add_chunk(summit_core::crypto::hash(&blocks[0]), blocks[0].clone())
            .await
            .unwrap();

        for _ in 0..2 {
            let (target, chunk) = rx.try_recv().expect("FILE_DONE queued");
            assert!(matches!(target, SendTarget::Peer { public_key } if public_key == [9; 32]));
            assert_eq!(chunk.type_tag, wire::recovery::FILE_DONE);
            let done: FileDone = serde_json::from_slice(&chunk.payload).unwrap();
            assert_eq!(done.metadata_hash, id);
            // Sender re-sends metadata (our FILE_DONE was lost): answer again,
            // without re-creating the assembly.
            reassembler
                .add_metadata(id, meta.clone(), [9; 32])
                .await
                .unwrap();
            assert!(reassembler.in_progress().await.is_empty());
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn outgoing_resends_until_confirmed_then_stops() {
        let reassembler = FileReassembler::new(temp_dir("reasm-out"));
        let chunks = chunk_bytes("o.bin", b"out").unwrap();
        let id = summit_core::crypto::hash(&chunks[0].payload);
        let (p1, p2) = ([1u8; 32], [2u8; 32]);
        reassembler
            .track_outgoing(chunks[0].clone(), vec![p1, p2])
            .await;

        // Not due before the interval.
        assert!(reassembler
            .due_metadata_resends(Duration::from_secs(60))
            .await
            .is_empty());
        let due = reassembler.due_metadata_resends(Duration::ZERO).await;
        assert_eq!(due.len(), 1);
        assert_eq!(due[0].1.len(), 2);

        reassembler.confirm_outgoing(&id, &p1).await;
        let due = reassembler.due_metadata_resends(Duration::ZERO).await;
        assert_eq!(due[0].1, vec![p2]);

        reassembler.confirm_outgoing(&id, &p2).await;
        assert_eq!(reassembler.unconfirmed_outgoing().await, 0);
    }

    #[tokio::test]
    async fn outgoing_gives_up_after_max_sends() {
        let reassembler = FileReassembler::new(temp_dir("reasm-giveup"));
        let chunks = chunk_bytes("g.bin", b"x").unwrap();
        reassembler
            .track_outgoing(chunks[0].clone(), vec![[1; 32]])
            .await;
        let mut resends = 0;
        while !reassembler
            .due_metadata_resends(Duration::ZERO)
            .await
            .is_empty()
        {
            resends += 1;
        }
        assert_eq!(resends, (MAX_METADATA_SENDS - 1) as usize);
        assert_eq!(reassembler.unconfirmed_outgoing().await, 0);
    }
}
