//! Trust management — peer authorization and access control.
//!
//! Three-tier trust model:
//! - Blocked:    Sessions dropped, chunks rejected
//! - Untrusted:  Sessions exist, chunks buffered (default for new peers)
//! - Trusted:    Full access, chunks processed immediately
//!
//! Sessions auto-establish (Noise handshake completes) but chunks are only
//! processed from trusted peers. This allows public discovery while maintaining
//! user control over data flow.

use bytes::Bytes;
use dashmap::DashMap;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;

/// Trust level for a peer, keyed by their public key.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TrustLevel {
    /// Peer is blocked — drop sessions, reject all chunks
    Blocked,
    /// Peer is unknown — sessions allowed, chunks buffered
    Untrusted,
    /// Peer is trusted — full access, process chunks
    Trusted,
}

impl Default for TrustLevel {
    fn default() -> Self {
        Self::Untrusted
    }
}

/// Registry of trusted/blocked peers.
pub struct TrustRegistry {
    rules: Arc<DashMap<[u8; 32], TrustLevel>>,
}

impl TrustRegistry {
    pub fn new() -> Self {
        Self {
            rules: Arc::new(DashMap::new()),
        }
    }

    /// Check trust level for a peer. Returns Untrusted if no rule exists.
    pub fn check(&self, public_key: &[u8; 32]) -> TrustLevel {
        self.rules
            .get(public_key)
            .map(|r| *r.value())
            .unwrap_or(TrustLevel::Untrusted)
    }

    /// Mark a peer as trusted. Flushes any buffered chunks for processing.
    pub fn trust(&self, public_key: [u8; 32]) {
        self.rules.insert(public_key, TrustLevel::Trusted);
        tracing::info!(peer = hex::encode(public_key), "peer trusted");
    }

    /// Mark a peer as blocked. Existing sessions will be dropped.
    pub fn block(&self, public_key: [u8; 32]) {
        self.rules.insert(public_key, TrustLevel::Blocked);
        tracing::info!(peer = hex::encode(public_key), "peer blocked");
    }

    /// Remove trust rule, reverting to default (Untrusted).
    pub fn remove(&self, public_key: &[u8; 32]) {
        self.rules.remove(public_key);
    }

    /// List all peers with explicit trust rules.
    pub fn list(&self) -> Vec<([u8; 32], TrustLevel)> {
        self.rules
            .iter()
            .map(|entry| (*entry.key(), *entry.value()))
            .collect()
    }

    /// Count peers by trust level.
    pub fn counts(&self) -> (usize, usize, usize) {
        let mut trusted = 0;
        let mut untrusted = 0;
        let mut blocked = 0;

        for entry in self.rules.iter() {
            match *entry.value() {
                TrustLevel::Trusted => trusted += 1,
                TrustLevel::Untrusted => untrusted += 1,
                TrustLevel::Blocked => blocked += 1,
            }
        }

        (trusted, untrusted, blocked)
    }
}

impl Clone for TrustRegistry {
    fn clone(&self) -> Self {
        Self {
            rules: self.rules.clone(),
        }
    }
}

/// Buffer for chunks from untrusted peers.
pub struct UntrustedBuffer {
    /// Map: peer_pubkey -> Vec<(content_hash, chunk_data)>
    buffer: Arc<DashMap<[u8; 32], Vec<([u8; 32], Bytes)>>>,
}

impl UntrustedBuffer {
    pub fn new() -> Self {
        Self {
            buffer: Arc::new(DashMap::new()),
        }
    }

    /// Add a chunk from an untrusted peer.
    pub fn add(&self, peer_pubkey: [u8; 32], content_hash: [u8; 32], data: Bytes) {
        self.buffer
            .entry(peer_pubkey)
            .or_insert_with(Vec::new)
            .push((content_hash, data));
    }

    /// Retrieve and remove all buffered chunks for a peer (when they become trusted).
    pub fn flush(&self, peer_pubkey: &[u8; 32]) -> Vec<([u8; 32], Bytes)> {
        self.buffer
            .remove(peer_pubkey)
            .map(|(_, chunks)| chunks)
            .unwrap_or_default()
    }

    /// Count buffered chunks for a peer.
    pub fn count(&self, peer_pubkey: &[u8; 32]) -> usize {
        self.buffer
            .get(peer_pubkey)
            .map(|chunks| chunks.len())
            .unwrap_or(0)
    }

    /// Total buffered chunks across all untrusted peers.
    pub fn total(&self) -> usize {
        self.buffer.iter().map(|entry| entry.value().len()).sum()
    }

    /// List peers with buffered chunks.
    pub fn peers(&self) -> Vec<([u8; 32], usize)> {
        self.buffer
            .iter()
            .map(|entry| (*entry.key(), entry.value().len()))
            .collect()
    }

    /// Clear all buffered chunks for a peer.
    pub fn clear(&self, peer_pubkey: &[u8; 32]) {
        self.buffer.remove(peer_pubkey);
    }
}

impl Clone for UntrustedBuffer {
    fn clone(&self) -> Self {
        Self {
            buffer: self.buffer.clone(),
        }
    }
}
