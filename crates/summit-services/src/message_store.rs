use dashmap::DashMap;
use std::sync::Arc;
use summit_core::message::{
    MessageChunk, MessageContent, MessageEnvelope, MessageMetadata, MessageType,
};

/// In-memory message store
#[derive(Clone, Default)]
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
        self.messages.entry(peer_pubkey).or_default().push(message);
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

    /// Store a message from a parsed JSON envelope.
    ///
    /// Converts the MessageEnvelope (JSON wire format) into a MessageChunk
    /// (internal storage format) and stores it keyed by peer_pubkey.
    pub fn add_from_envelope(
        &self,
        peer_pubkey: &[u8; 32],
        envelope: &MessageEnvelope,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        // Derive msg_id from the envelope content
        let envelope_bytes = serde_json::to_vec(envelope)?;
        let msg_id = summit_core::crypto::hash(&envelope_bytes);

        // Parse sender hex back to [u8; 32]
        let sender_bytes = hex::decode(&envelope.sender)?;
        let mut sender = [0u8; 32];
        if sender_bytes.len() == 32 {
            sender.copy_from_slice(&sender_bytes);
        }

        // Map msg_type string to MessageType enum
        let msg_type = match envelope.msg_type.as_str() {
            "text" | "ack" | "read" | _ => MessageType::Text,
        };

        // Extract text content from payload
        let content = match envelope.msg_type.as_str() {
            "text" => {
                let body = envelope
                    .payload
                    .get("body")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();
                MessageContent::Text { text: body }
            }
            _ => {
                // Store raw JSON for unknown types
                MessageContent::Text {
                    text: envelope.payload.to_string(),
                }
            }
        };

        let chunk = MessageChunk {
            msg_id,
            msg_type,
            timestamp: envelope.timestamp,
            sender,
            recipient: *peer_pubkey, // we are the recipient
            content,
            metadata: MessageMetadata {
                mime_type: None,
                size_bytes: None,
                filename: None,
                dimensions: None,
                duration_secs: None,
            },
        };

        self.add(*peer_pubkey, chunk);
        Ok(())
    }

    /// Clear all messages
    pub fn clear(&self) {
        self.messages.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use summit_core::message::{MessageContent, MessageMetadata, MessageType};

    fn make_msg(timestamp: u64) -> MessageChunk {
        MessageChunk {
            msg_id: [0u8; 32],
            msg_type: MessageType::Text,
            timestamp,
            sender: [1u8; 32],
            recipient: [2u8; 32],
            content: MessageContent::Text {
                text: "hello".into(),
            },
            metadata: MessageMetadata {
                mime_type: None,
                size_bytes: None,
                filename: None,
                dimensions: None,
                duration_secs: None,
            },
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
        store.add(peer, make_msg(100));
        store.add(peer, make_msg(200));

        let msgs = store.get(&peer);
        assert_eq!(msgs.len(), 2);
        assert_eq!(msgs[0].timestamp, 100);
        assert_eq!(msgs[1].timestamp, 200);
    }

    #[test]
    fn get_since_filters_by_timestamp() {
        let store = MessageStore::new();
        let peer = [1u8; 32];
        store.add(peer, make_msg(100));
        store.add(peer, make_msg(200));
        store.add(peer, make_msg(300));

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
        store.add(peer, make_msg(100));
        assert_eq!(store.count(&peer), 1);
        store.add(peer, make_msg(200));
        assert_eq!(store.count(&peer), 2);
    }

    #[test]
    fn clear_wipes_all_messages() {
        let store = MessageStore::new();
        let peer_a = [1u8; 32];
        let peer_b = [2u8; 32];
        store.add(peer_a, make_msg(100));
        store.add(peer_b, make_msg(200));

        store.clear();
        assert_eq!(store.count(&peer_a), 0);
        assert_eq!(store.count(&peer_b), 0);
    }
}
