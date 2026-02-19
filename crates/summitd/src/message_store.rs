use dashmap::DashMap;
use std::sync::Arc;
use summit_core::message::MessageChunk;

/// In-memory message store
#[derive(Clone)]
pub struct MessageStore {
    /// Messages per peer: peer_pubkey -> Vec<MessageChunk>
    messages: Arc<DashMap<[u8; 32], Vec<MessageChunk>>>,
}

impl MessageStore {
    pub fn new() -> Self {
        Self {
            messages: Arc::new(DashMap::new()),
        }
    }

    /// Add a message
    pub fn add(&self, peer_pubkey: [u8; 32], message: MessageChunk) {
        self.messages
            .entry(peer_pubkey)
            .or_insert_with(Vec::new)
            .push(message);
    }

    /// Get all messages with a peer
    pub fn get(&self, peer_pubkey: &[u8; 32]) -> Vec<MessageChunk> {
        self.messages
            .get(peer_pubkey)
            .map(|msgs| msgs.clone())
            .unwrap_or_default()
    }

    /// Get messages since timestamp
    pub fn get_since(&self, peer_pubkey: &[u8; 32], since: u64) -> Vec<MessageChunk> {
        self.messages
            .get(peer_pubkey)
            .map(|msgs| {
                msgs.iter()
                    .filter(|m| m.timestamp > since)
                    .cloned()
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Count messages with a peer
    pub fn count(&self, peer_pubkey: &[u8; 32]) -> usize {
        self.messages
            .get(peer_pubkey)
            .map(|msgs| msgs.len())
            .unwrap_or(0)
    }

    /// Clear all messages
    pub fn clear(&self) {
        self.messages.clear();
    }
}
