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
use std::sync::Arc;

/// Trust level for a peer, keyed by their public key.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TrustLevel {
    /// Peer is blocked — drop sessions, reject all chunks
    Blocked,
    /// Peer is unknown — sessions allowed, chunks buffered
    #[default]
    Untrusted,
    /// Peer is trusted — full access, process chunks
    Trusted,
}

/// Registry of trusted/blocked peers.
pub struct TrustRegistry {
    rules: Arc<DashMap<[u8; 32], TrustLevel>>,
    auto_trust: Arc<std::sync::atomic::AtomicBool>,
}

impl Default for TrustRegistry {
    fn default() -> Self {
        Self::new()
    }
}

impl TrustRegistry {
    pub fn new() -> Self {
        Self {
            rules: Arc::new(DashMap::new()),
            auto_trust: Arc::new(std::sync::atomic::AtomicBool::new(false)),
        }
    }

    /// Apply config: auto-trust setting and pre-trusted peer keys.
    pub fn apply_config(&self, auto_trust: bool, trusted_peers: &[String]) {
        self.auto_trust
            .store(auto_trust, std::sync::atomic::Ordering::Relaxed);

        for hex_key in trusted_peers {
            if let Ok(bytes) = hex::decode(hex_key) {
                if bytes.len() == 32 {
                    let mut key = [0u8; 32];
                    key.copy_from_slice(&bytes);
                    self.trust(key);
                    tracing::info!(
                        peer = &hex_key[..16.min(hex_key.len())],
                        "pre-trusted peer from config"
                    );
                }
            }
        }
    }

    /// Check if a peer should be trusted.
    /// Returns true if auto_trust is on OR the peer is explicitly trusted.
    pub fn is_trusted(&self, peer_pubkey: &[u8; 32]) -> bool {
        if self
            .auto_trust
            .load(std::sync::atomic::Ordering::Relaxed)
        {
            return true;
        }
        self.rules
            .get(peer_pubkey)
            .map(|r| matches!(*r.value(), TrustLevel::Trusted))
            .unwrap_or(false)
    }

    /// Check trust level for a peer. Returns Untrusted if no rule exists.
    pub fn check(&self, public_key: &[u8; 32]) -> TrustLevel {
        if self.auto_trust.load(std::sync::atomic::Ordering::Relaxed) {
            // In auto-trust mode, return Trusted unless explicitly blocked
            let level = self
                .rules
                .get(public_key)
                .map(|r| *r.value())
                .unwrap_or(TrustLevel::Trusted);
            return if matches!(level, TrustLevel::Blocked) {
                TrustLevel::Blocked
            } else {
                TrustLevel::Trusted
            };
        }
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
            auto_trust: self.auto_trust.clone(),
        }
    }
}

/// A buffered chunk: (content_hash, chunk_data).
type BufferedChunk = ([u8; 32], Bytes);

/// Buffer for chunks from untrusted peers.
pub struct UntrustedBuffer {
    /// Map: peer_pubkey -> Vec<BufferedChunk>
    buffer: Arc<DashMap<[u8; 32], Vec<BufferedChunk>>>,
}

impl Default for UntrustedBuffer {
    fn default() -> Self {
        Self::new()
    }
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
            .or_default()
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_trust_level_is_untrusted() {
        assert_eq!(TrustLevel::default(), TrustLevel::Untrusted);
    }

    #[test]
    fn trust_block_check_roundtrip() {
        let reg = TrustRegistry::new();
        let peer = [1u8; 32];

        assert_eq!(reg.check(&peer), TrustLevel::Untrusted);

        reg.trust(peer);
        assert_eq!(reg.check(&peer), TrustLevel::Trusted);

        reg.block(peer);
        assert_eq!(reg.check(&peer), TrustLevel::Blocked);
    }

    #[test]
    fn remove_reverts_to_untrusted() {
        let reg = TrustRegistry::new();
        let peer = [1u8; 32];

        reg.trust(peer);
        assert_eq!(reg.check(&peer), TrustLevel::Trusted);

        reg.remove(&peer);
        assert_eq!(reg.check(&peer), TrustLevel::Untrusted);
    }

    #[test]
    fn list_and_counts() {
        let reg = TrustRegistry::new();
        reg.trust([1u8; 32]);
        reg.trust([2u8; 32]);
        reg.block([3u8; 32]);

        let list = reg.list();
        assert_eq!(list.len(), 3);

        let (trusted, untrusted, blocked) = reg.counts();
        assert_eq!(trusted, 2);
        assert_eq!(untrusted, 0);
        assert_eq!(blocked, 1);
    }

    #[test]
    fn untrusted_buffer_add_and_flush() {
        let buf = UntrustedBuffer::new();
        let peer = [1u8; 32];
        let hash = [2u8; 32];

        buf.add(peer, hash, Bytes::from_static(b"data"));
        assert_eq!(buf.count(&peer), 1);

        let flushed = buf.flush(&peer);
        assert_eq!(flushed.len(), 1);
        assert_eq!(flushed[0].0, hash);
        assert_eq!(flushed[0].1, Bytes::from_static(b"data"));

        // After flush, count should be 0
        assert_eq!(buf.count(&peer), 0);
    }

    #[test]
    fn untrusted_buffer_total_and_peers() {
        let buf = UntrustedBuffer::new();
        let peer_a = [1u8; 32];
        let peer_b = [2u8; 32];

        buf.add(peer_a, [10u8; 32], Bytes::from_static(b"a1"));
        buf.add(peer_a, [11u8; 32], Bytes::from_static(b"a2"));
        buf.add(peer_b, [20u8; 32], Bytes::from_static(b"b1"));

        assert_eq!(buf.total(), 3);

        let peers = buf.peers();
        assert_eq!(peers.len(), 2);
    }

    #[test]
    fn untrusted_buffer_clear() {
        let buf = UntrustedBuffer::new();
        let peer = [1u8; 32];

        buf.add(peer, [10u8; 32], Bytes::from_static(b"data"));
        assert_eq!(buf.count(&peer), 1);

        buf.clear(&peer);
        assert_eq!(buf.count(&peer), 0);
    }
}
