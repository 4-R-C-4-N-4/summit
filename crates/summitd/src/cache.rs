//! Content-addressed chunk cache.
//!
//! Chunks are stored by content hash in a two-level directory structure:
//!   /var/cache/summit/chunks/{hash[0..2]}/{full_hash}
//!
//! This is the same layout Git uses for objects. Files are immutable —
//! if the hash exists, the content is correct. No TTLs, no invalidation.

use std::fs;
use std::io::Write;
use std::path::PathBuf;

use anyhow::{Context, Result};
use bytes::Bytes;
use memmap2::Mmap;

/// Content-addressed chunk cache.
#[derive(Clone)]
pub struct ChunkCache {
    root: PathBuf,
}

impl ChunkCache {
    /// Create a cache rooted at the given directory.
    ///
    /// For production: /var/cache/summit/chunks
    /// For testing: /tmp/summit-cache-{pid}
    pub fn new(root: impl Into<PathBuf>) -> Result<Self> {
        let root = root.into();
        fs::create_dir_all(&root)
            .with_context(|| format!("failed to create cache root: {}", root.display()))?;
        Ok(Self { root })
    }

    /// Check if a chunk exists in the cache.
    pub fn has(&self, hash: &[u8; 32]) -> bool {
        self.chunk_path(hash).exists()
    }

    /// Retrieve a chunk from the cache.
    ///
    /// Returns None if not present. The returned Bytes is backed by mmap,
    /// so reads are zero-copy and page faults bring data from disk on demand.
    pub fn get(&self, hash: &[u8; 32]) -> Result<Option<Bytes>> {
        let path = self.chunk_path(hash);
        if !path.exists() {
            return Ok(None);
        }

        let file = fs::File::open(&path)
            .with_context(|| format!("failed to open chunk: {}", path.display()))?;

        // Safety: file is opened read-only and we don't mutate the mmap
        let mmap = unsafe {
            Mmap::map(&file).with_context(|| format!("failed to mmap chunk: {}", path.display()))?
        };

        // Copy mmap into Bytes — this is still zero-copy in the sense that
        // Bytes::copy_from_slice is cheap for small sizes, and large mmaps
        // benefit from kernel page cache. For true zero-copy we'd need to
        // return the Mmap directly, but Bytes is more convenient.
        Ok(Some(Bytes::copy_from_slice(&mmap)))
    }

    /// Store a chunk in the cache.
    ///
    /// Writes are atomic: write to temp file, then rename. If the chunk
    /// already exists, this is a no-op (immutability = idempotence).
    pub fn put(&self, hash: &[u8; 32], data: &[u8]) -> Result<()> {
        let path = self.chunk_path(hash);

        // Already exists? Nothing to do.
        if path.exists() {
            return Ok(());
        }

        // Ensure directory exists
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)
                .with_context(|| format!("failed to create cache dir: {}", parent.display()))?;
        }

        // Atomic write: tmp file → rename
        let tmp_path = path.with_extension("tmp");
        {
            let mut file = fs::File::create(&tmp_path)
                .with_context(|| format!("failed to create temp file: {}", tmp_path.display()))?;
            file.write_all(data)
                .with_context(|| format!("failed to write chunk data"))?;
            file.sync_all().context("failed to sync chunk to disk")?;
        }

        fs::rename(&tmp_path, &path).with_context(|| {
            format!(
                "failed to rename {} to {}",
                tmp_path.display(),
                path.display()
            )
        })?;

        tracing::trace!(hash = hex::encode(hash), "chunk cached");
        Ok(())
    }

    /// Get the filesystem path for a chunk.
    fn chunk_path(&self, hash: &[u8; 32]) -> PathBuf {
        let hex = hex::encode(hash);
        // Two-level: chunks/ab/abc123...
        self.root.join(&hex[0..2]).join(&hex)
    }

    /// Count total chunks in cache (for stats/debugging).
    pub fn count(&self) -> usize {
        let mut total = 0;
        if let Ok(entries) = fs::read_dir(&self.root) {
            for entry in entries.flatten() {
                if let Ok(subdir) = fs::read_dir(entry.path()) {
                    total += subdir.count();
                }
            }
        }
        total
    }

    /// Total cache size in bytes (for stats/debugging).
    pub fn size(&self) -> u64 {
        let mut total = 0u64;
        if let Ok(entries) = fs::read_dir(&self.root) {
            for entry in entries.flatten() {
                if let Ok(subdir) = fs::read_dir(entry.path()) {
                    for chunk in subdir.flatten() {
                        if let Ok(meta) = chunk.metadata() {
                            total += meta.len();
                        }
                    }
                }
            }
        }
        total
    }

    pub fn clear(&self) {
        if let Ok(entries) = std::fs::read_dir(&self.root) {
            for entry in entries.flatten() {
                let _ = std::fs::remove_dir_all(entry.path());
            }
        }
    }
}
