//! Delivery tracking — records which chunks arrived from which peers.

use std::sync::Arc;

use dashmap::DashMap;
use tokio::time::Instant;

/// Tracks chunk deliveries for multipath analysis.
#[derive(Clone)]
pub struct DeliveryTracker {
    // content_hash -> Vec<(peer_addr, received_at)>
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
            .or_insert_with(Vec::new)
            .push((peer_addr, Instant::now()));
    }

    /// Get all deliveries for a chunk.
    pub fn get(&self, content_hash: &[u8; 32]) -> Option<Vec<(String, Instant)>> {
        self.deliveries.get(content_hash).map(|v| v.clone())
    }

    /// Count how many peers delivered this chunk.
    pub fn delivery_count(&self, content_hash: &[u8; 32]) -> usize {
        self.deliveries
            .get(content_hash)
            .map(|v| v.len())
            .unwrap_or(0)
    }

    /// Print delivery stats for chunks with multiple paths.
    pub fn print_stats(&self) {
        let multipath_count = self
            .deliveries
            .iter()
            .filter(|entry| entry.value().len() > 1)
            .count();

        if multipath_count > 0 {
            tracing::info!(
                total_chunks = self.deliveries.len(),
                multipath_chunks = multipath_count,
                "delivery tracker stats"
            );

            for entry in self.deliveries.iter() {
                let deliveries = entry.value();
                if deliveries.len() > 1 {
                    let hash = hex::encode(entry.key());
                    let peers: Vec<_> = deliveries.iter().map(|(p, _)| p.clone()).collect();

                    tracing::info!(
                        chunk = &hash[..16],
                        paths = deliveries.len(),
                        peers = ?peers,
                        "multipath delivery detected"
                    );
                }
            }
        }
    }
}
