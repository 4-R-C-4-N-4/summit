//! Delivery tracking — records which chunks arrived from which peers.

use std::sync::Arc;
use std::time::Duration;

use dashmap::DashMap;
use tokio::time::Instant;

/// How long to keep delivery records before eviction.
const DELIVERY_TTL: Duration = Duration::from_secs(120);

/// Tracks chunk deliveries for multipath analysis.
#[derive(Clone)]
pub struct DeliveryTracker {
    // content_hash -> Vec<(peer_addr, received_at)>
    #[allow(clippy::type_complexity)]
    deliveries: Arc<DashMap<[u8; 32], Vec<(String, Instant)>>>,
}

impl DeliveryTracker {
    pub fn new() -> Self {
        Self {
            deliveries: Arc::new(DashMap::new()),
        }
    }

    /// Record a chunk delivery from a peer.
    pub fn record(&self, content_hash: [u8; 32], peer_addr: String) {
        self.deliveries
            .entry(content_hash)
            .or_default()
            .push((peer_addr, Instant::now()));
    }

    /// Count how many times this chunk was delivered (including retransmissions).
    pub fn delivery_count(&self, content_hash: &[u8; 32]) -> usize {
        self.deliveries
            .get(content_hash)
            .map(|v| v.len())
            .unwrap_or(0)
    }

    /// Evict entries older than TTL. Call periodically to bound memory.
    pub fn evict_expired(&self) {
        let before = self.deliveries.len();
        self.deliveries.retain(|_, entries| {
            entries
                .last()
                .map(|(_, t)| t.elapsed() < DELIVERY_TTL)
                .unwrap_or(false)
        });
        let removed = before.saturating_sub(self.deliveries.len());
        if removed > 0 {
            tracing::debug!(
                removed,
                remaining = self.deliveries.len(),
                "delivery tracker eviction"
            );
        }
    }

    /// Print delivery stats for chunks with multiple deliveries.
    pub fn print_stats(&self) {
        use std::collections::HashSet;

        let mut multipath_count = 0;
        let mut retransmit_count = 0;

        for entry in self.deliveries.iter() {
            let deliveries = entry.value();
            if deliveries.len() > 1 {
                let unique_peers: HashSet<_> = deliveries.iter().map(|(p, _)| p.as_str()).collect();
                if unique_peers.len() > 1 {
                    multipath_count += 1;
                } else {
                    retransmit_count += 1;
                }
            }
        }

        if multipath_count > 0 || retransmit_count > 0 {
            tracing::info!(
                total_chunks = self.deliveries.len(),
                multipath_chunks = multipath_count,
                retransmitted_chunks = retransmit_count,
                "delivery tracker stats"
            );
        }

        // Only log individual entries for true multipath (different peers)
        for entry in self.deliveries.iter() {
            let deliveries = entry.value();
            if deliveries.len() > 1 {
                let unique_peers: HashSet<_> = deliveries.iter().map(|(p, _)| p.as_str()).collect();
                if unique_peers.len() > 1 {
                    let hash = hex::encode(entry.key());
                    let peers: Vec<_> = unique_peers.into_iter().collect();
                    tracing::info!(
                        chunk = &hash[..16],
                        unique_paths = peers.len(),
                        total_deliveries = deliveries.len(),
                        peers = ?peers,
                        "multipath delivery detected"
                    );
                }
            }
        }
    }
}
