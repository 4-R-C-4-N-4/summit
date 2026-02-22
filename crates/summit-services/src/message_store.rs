use crate::messaging_service::MessageEnvelope;
use dashmap::DashMap;
use std::sync::Arc;

/// In-memory store for received message envelopes, keyed by sender pubkey.
#[derive(Clone, Default)]
pub struct MessageStore {
    messages: Arc<DashMap<[u8; 32], Vec<MessageEnvelope>>>,
}

impl MessageStore {
    pub fn new() -> Self {
        Self {
            messages: Arc::new(DashMap::new()),
        }
    }

    /// Store an envelope received from `peer_pubkey`.
    pub fn add(&self, peer_pubkey: [u8; 32], envelope: MessageEnvelope) {
        self.messages.entry(peer_pubkey).or_default().push(envelope);
    }

    /// Get all envelopes received from `peer_pubkey`.
    pub fn get(&self, peer_pubkey: &[u8; 32]) -> Vec<MessageEnvelope> {
        self.messages
            .get(peer_pubkey)
            .map(|msgs| msgs.clone())
            .unwrap_or_default()
    }

    /// Get envelopes received from `peer_pubkey` with `timestamp > since`.
    pub fn get_since(&self, peer_pubkey: &[u8; 32], since: u64) -> Vec<MessageEnvelope> {
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

    /// Count envelopes stored for `peer_pubkey`.
    pub fn count(&self, peer_pubkey: &[u8; 32]) -> usize {
        self.messages
            .get(peer_pubkey)
            .map(|msgs| msgs.len())
            .unwrap_or(0)
    }

    /// Clear all stored messages.
    pub fn clear(&self) {
        self.messages.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_envelope(timestamp: u64) -> MessageEnvelope {
        MessageEnvelope {
            msg_id: format!("id-{}", timestamp),
            msg_type: "text".into(),
            sender: "a".repeat(64),
            timestamp,
            payload: serde_json::json!({ "text": "hello" }),
        }
    }

    #[test]
    fn new_creates_empty_store() {
        let store = MessageStore::new();
        let peer = [1u8; 32];
        assert_eq!(store.count(&peer), 0);
        assert!(store.get(&peer).is_empty());
    }

    #[test]
    fn add_and_get_roundtrip() {
        let store = MessageStore::new();
        let peer = [1u8; 32];
        store.add(peer, make_envelope(100));
        store.add(peer, make_envelope(200));

        let msgs = store.get(&peer);
        assert_eq!(msgs.len(), 2);
        assert_eq!(msgs[0].timestamp, 100);
        assert_eq!(msgs[1].timestamp, 200);
    }

    #[test]
    fn get_since_filters_by_timestamp() {
        let store = MessageStore::new();
        let peer = [1u8; 32];
        store.add(peer, make_envelope(100));
        store.add(peer, make_envelope(200));
        store.add(peer, make_envelope(300));

        let msgs = store.get_since(&peer, 150);
        assert_eq!(msgs.len(), 2);
        assert_eq!(msgs[0].timestamp, 200);
        assert_eq!(msgs[1].timestamp, 300);
    }

    #[test]
    fn count_returns_correct_count() {
        let store = MessageStore::new();
        let peer = [1u8; 32];
        assert_eq!(store.count(&peer), 0);
        store.add(peer, make_envelope(100));
        assert_eq!(store.count(&peer), 1);
        store.add(peer, make_envelope(200));
        assert_eq!(store.count(&peer), 2);
    }

    #[test]
    fn clear_wipes_all_messages() {
        let store = MessageStore::new();
        let peer_a = [1u8; 32];
        let peer_b = [2u8; 32];
        store.add(peer_a, make_envelope(100));
        store.add(peer_b, make_envelope(200));

        store.clear();
        assert_eq!(store.count(&peer_a), 0);
        assert_eq!(store.count(&peer_b), 0);
    }
}
