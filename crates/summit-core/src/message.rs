//! Message schema for rich P2P messaging
//! 
//! Supports text, images, videos, audio, files, reactions, edits, and typing indicators.

use serde::{Deserialize, Serialize};

/// Message type discriminator
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[repr(u8)]
pub enum MessageType {
    Text = 1,
    Image = 2,
    Video = 3,
    Audio = 4,
    File = 5,
    Reaction = 6,
    Edit = 7,
    Delete = 8,
    Typing = 9,
}

impl MessageType {
    pub fn from_u8(value: u8) -> Option<Self> {
        match value {
            1 => Some(Self::Text),
            2 => Some(Self::Image),
            3 => Some(Self::Video),
            4 => Some(Self::Audio),
            5 => Some(Self::File),
            6 => Some(Self::Reaction),
            7 => Some(Self::Edit),
            8 => Some(Self::Delete),
            9 => Some(Self::Typing),
            _ => None,
        }
    }
}

/// Message metadata
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MessageMetadata {
    /// MIME type (e.g., "image/jpeg", "video/mp4")
    pub mime_type: Option<String>,
    
    /// File size in bytes
    pub size_bytes: Option<u64>,
    
    /// Original filename
    pub filename: Option<String>,
    
    /// Image/video dimensions
    pub dimensions: Option<(u32, u32)>,
    
    /// Audio/video duration in seconds
    pub duration_secs: Option<f32>,
}

/// Message content variants
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", content = "data")]
pub enum MessageContent {
    /// Plain text message
    Text {
        text: String,
    },
    
    /// Media (image/video/audio) with optional preview
    Media {
        /// BLAKE3 hash of the media content
        content_hash: [u8; 32],
        
        /// Number of chunks for this media
        chunk_count: u32,
        
        /// Base64-encoded thumbnail/preview (for images/videos)
        /// Max 32KB for efficient transmission
        preview: Option<Vec<u8>>,
    },
    
    /// File attachment
    File {
        /// BLAKE3 hash of the file content
        content_hash: [u8; 32],
        
        /// Number of chunks
        chunk_count: u32,
    },
    
    /// Emoji reaction to another message
    Reaction {
        /// Message ID being reacted to
        target_msg_id: [u8; 32],
        
        /// Unicode emoji (e.g., "👍", "❤️")
        emoji: String,
    },
    
    /// Edit to a previous message
    Edit {
        /// Original message ID
        original_msg_id: [u8; 32],
        
        /// New content
        new_content: String,
    },
    
    /// Delete/retract a message
    Delete {
        /// Message ID to delete
        target_msg_id: [u8; 32],
    },
    
    /// Typing indicator (ephemeral, not stored)
    Typing {
        /// Whether user is typing (true) or stopped (false)
        is_typing: bool,
    },
}

/// Complete message chunk structure
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MessageChunk {
    /// Unique message ID (BLAKE3 hash of content + timestamp + sender)
    pub msg_id: [u8; 32],
    
    /// Message type
    pub msg_type: MessageType,
    
    /// Unix timestamp in milliseconds
    pub timestamp: u64,
    
    /// Sender's public key
    pub sender: [u8; 32],
    
    /// Recipient's public key (for DMs)
    pub recipient: [u8; 32],
    
    /// Message content
    pub content: MessageContent,
    
    /// Metadata
    pub metadata: MessageMetadata,
}

impl MessageChunk {
    /// Create a new text message
    pub fn text(
        sender: [u8; 32],
        recipient: [u8; 32],
        text: String,
    ) -> Self {
        let timestamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis() as u64;
        
        let msg_id = Self::generate_id(&sender, &recipient, timestamp, &text);
        
        Self {
            msg_id,
            msg_type: MessageType::Text,
            timestamp,
            sender,
            recipient,
            content: MessageContent::Text { text },
            metadata: MessageMetadata {
                mime_type: Some("text/plain".to_string()),
                size_bytes: None,
                filename: None,
                dimensions: None,
                duration_secs: None,
            },
        }
    }
    
    /// Create a media message (image/video/audio)
    pub fn media(
        sender: [u8; 32],
        recipient: [u8; 32],
        msg_type: MessageType,
        content_hash: [u8; 32],
        chunk_count: u32,
        preview: Option<Vec<u8>>,
        metadata: MessageMetadata,
    ) -> Self {
        let timestamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis() as u64;
        
        let msg_id = Self::generate_id(&sender, &recipient, timestamp, &content_hash);
        
        Self {
            msg_id,
            msg_type,
            timestamp,
            sender,
            recipient,
            content: MessageContent::Media {
                content_hash,
                chunk_count,
                preview,
            },
            metadata,
        }
    }
    
    /// Create a reaction
    pub fn reaction(
        sender: [u8; 32],
        recipient: [u8; 32],
        target_msg_id: [u8; 32],
        emoji: String,
    ) -> Self {
        let timestamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis() as u64;
        
        let msg_id = Self::generate_id(&sender, &recipient, timestamp, &emoji);
        
        Self {
            msg_id,
            msg_type: MessageType::Reaction,
            timestamp,
            sender,
            recipient,
            content: MessageContent::Reaction {
                target_msg_id,
                emoji,
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
    
    /// Generate message ID from content
    fn generate_id(
        sender: &[u8; 32],
        recipient: &[u8; 32],
        timestamp: u64,
        content: &impl AsRef<[u8]>,
    ) -> [u8; 32] {
        use blake3::Hasher;
        
        let mut hasher = Hasher::new();
        hasher.update(sender);
        hasher.update(recipient);
        hasher.update(&timestamp.to_le_bytes());
        hasher.update(content.as_ref());
        
        let hash = hasher.finalize();
        let mut id = [0u8; 32];
        id.copy_from_slice(hash.as_bytes());
        id
    }
    
    /// Serialize to bytes for transmission
    pub fn to_bytes(&self) -> Vec<u8> {
        serde_json::to_vec(self).expect("message serialization failed")
    }
    
    /// Deserialize from bytes
    pub fn from_bytes(data: &[u8]) -> Result<Self, serde_json::Error> {
        serde_json::from_slice(data)
    }
}

/// Message storage entry
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StoredMessage {
    pub message: MessageChunk,
    pub status: MessageStatus,
    pub media_path: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum MessageStatus {
    Sending,
    Sent,
    Delivered,
    Read,
    Failed,
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_text_message() {
        let sender = [1u8; 32];
        let recipient = [2u8; 32];
        let msg = MessageChunk::text(sender, recipient, "Hello!".to_string(), None);
        
        assert_eq!(msg.msg_type, MessageType::Text);
        assert_eq!(msg.sender, sender);
        assert_eq!(msg.recipient, recipient);
        
        match msg.content {
            MessageContent::Text { text } => assert_eq!(text, "Hello!"),
            _ => panic!("Expected text content"),
        }
    }
    
    #[test]
    fn test_serialization() {
        let sender = [1u8; 32];
        let recipient = [2u8; 32];
        let msg = MessageChunk::text(sender, recipient, "Test".to_string(), None);
        
        let bytes = msg.to_bytes();
        let decoded = MessageChunk::from_bytes(&bytes).unwrap();
        
        assert_eq!(msg.msg_id, decoded.msg_id);
        assert_eq!(msg.timestamp, decoded.timestamp);
    }
}
