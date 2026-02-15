//! Schema validation — ensures chunks conform to expected types.
//!
//! For Zenith, schemas are hard-coded Rust validators. A production
//! system would use WASM to allow dynamic schema loading.

use anyhow::{bail, Context, Result};

/// Known schema IDs (precomputed BLAKE3 hashes of schema names).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KnownSchema {
    /// summit.test.ping — simple ping messages
    TestPing,
    /// summit.message.text — UTF-8 text messages
    TextMessage,
    /// summit.file.chunk — arbitrary binary data
    FileChunk,
}

impl KnownSchema {

    pub fn from_id(schema_id: &[u8; 32]) -> Option<Self> {
        // Precomputed at build time
        let test_ping_id = summit_core::crypto::hash(b"summit.test.ping");
        let text_message_id = summit_core::crypto::hash(b"summit.message.text");
        let file_chunk_id = summit_core::crypto::hash(b"summit.file.chunk");

        if schema_id == &test_ping_id {
            Some(Self::TestPing)
        } else if schema_id == &text_message_id {
            Some(Self::TextMessage)
        } else if schema_id == &file_chunk_id {
            Some(Self::FileChunk)
        } else {
            None
        }
    }

    /// Validate a chunk payload against this schema.
    pub fn validate(&self, payload: &[u8]) -> Result<()> {
        match self {
            Self::TestPing => {
                let s = std::str::from_utf8(payload)
                    .context("ping payload must be UTF-8")?;
                
                if !s.starts_with("ping #") {
                    bail!("ping must start with 'ping #', got: {}", s);
                }
                
                Ok(())
            }
            
            Self::TextMessage => {
                std::str::from_utf8(payload)
                    .context("text message must be UTF-8")?;
                Ok(())
            }
            
            Self::FileChunk => {
                // No validation — arbitrary bytes allowed
                Ok(())
            }
        }
    }

    /// Get the schema ID (BLAKE3 hash).
    pub fn id(&self) -> [u8; 32] {
        match self {
            Self::TestPing => summit_core::crypto::hash(b"summit.test.ping"),
            Self::TextMessage => summit_core::crypto::hash(b"summit.message.text"),
            Self::FileChunk => summit_core::crypto::hash(b"summit.file.chunk"),
        }
    }

    /// Human-readable name.
    pub fn name(&self) -> &'static str {
        match self {
            Self::TestPing => "summit.test.ping",
            Self::TextMessage => "summit.message.text",
            Self::FileChunk => "summit.file.chunk",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ping_validation() {
        let schema = KnownSchema::TestPing;
        
        // Valid
        assert!(schema.validate(b"ping #1").is_ok());
        assert!(schema.validate(b"ping #999").is_ok());
        
        // Invalid
        assert!(schema.validate(b"pong #1").is_err());
        assert!(schema.validate(b"hello").is_err());
        assert!(schema.validate(&[0xFF, 0xFE]).is_err());
    }

    #[test]
    fn test_text_message_validation() {
        let schema = KnownSchema::TextMessage;
        
        // Valid
        assert!(schema.validate(b"hello world").is_ok());
        assert!(schema.validate(b"").is_ok());
        
        // Invalid
        assert!(schema.validate(&[0xFF, 0xFE]).is_err());
    }

    #[test]
    fn test_file_chunk_validation() {
        let schema = KnownSchema::FileChunk;
        
        // Everything is valid
        assert!(schema.validate(b"text").is_ok());
        assert!(schema.validate(&[0xFF, 0xFE, 0xFD]).is_ok());
        assert!(schema.validate(b"").is_ok());
    }

    #[test]
    fn test_schema_id_roundtrip() {
        let schema = KnownSchema::TestPing;
        let id = schema.id();
        let recovered = KnownSchema::from_id(&id).unwrap();
        assert_eq!(schema, recovered);
    }
}
