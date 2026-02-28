//! Schema validation — ensures chunks conform to expected types.

use anyhow::{bail, Context, Result};

use crate::file_transfer::FileMetadata;

/// Known schema IDs (precomputed BLAKE3 hashes of schema names).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KnownSchema {
    TestPing,
    Message,
    /// summit.file.chunk — arbitrary binary data
    FileChunk,
    FileData,
    FileMetadata,
    ComputeTask,
    Recovery,
}

impl KnownSchema {
    pub fn from_id(schema_id: &[u8; 32]) -> Option<Self> {
        let test_ping_id = summit_core::crypto::hash(b"summit.test.ping");
        let file_chunk_id = summit_core::crypto::hash(b"summit.file.chunk");
        let file_data_id = summit_core::crypto::hash(b"summit.file.data");
        let file_metadata_id = summit_core::crypto::hash(b"summit.file.metadata");
        let message_id = summit_core::wire::messaging_hash();
        let compute_id = summit_core::wire::compute_hash();
        let recovery_id = summit_core::wire::recovery_hash();

        if schema_id == &test_ping_id {
            Some(Self::TestPing)
        } else if schema_id == &file_chunk_id {
            Some(Self::FileChunk)
        } else if schema_id == &file_data_id {
            Some(Self::FileData)
        } else if schema_id == &file_metadata_id {
            Some(Self::FileMetadata)
        } else if schema_id == &message_id {
            Some(Self::Message)
        } else if schema_id == &compute_id {
            Some(Self::ComputeTask)
        } else if schema_id == &recovery_id {
            Some(Self::Recovery)
        } else {
            None
        }
    }

    /// Validate a chunk payload against this schema.
    pub fn validate(&self, payload: &[u8]) -> Result<()> {
        match self {
            Self::TestPing => {
                let s = std::str::from_utf8(payload).context("ping payload must be UTF-8")?;

                if !s.starts_with("ping #") {
                    bail!("ping must start with 'ping #', got: {}", s);
                }

                Ok(())
            }

            Self::FileChunk => Ok(()),
            Self::FileData => Ok(()),
            Self::FileMetadata => {
                serde_json::from_slice::<FileMetadata>(payload)
                    .context("invalid file metadata JSON")?;
                Ok(())
            }
            Self::Message => Ok(()),
            Self::ComputeTask => {
                serde_json::from_slice::<crate::compute_types::ComputeEnvelope>(payload)
                    .context("invalid compute envelope JSON")?;
                Ok(())
            }
            Self::Recovery => Ok(()), // Validated by the handler
        }
    }

    /// Get the schema ID (BLAKE3 hash).
    pub fn id(&self) -> [u8; 32] {
        match self {
            Self::TestPing => summit_core::crypto::hash(b"summit.test.ping"),
            Self::FileChunk => summit_core::crypto::hash(b"summit.file.chunk"),
            Self::FileData => summit_core::crypto::hash(b"summit.file.data"),
            Self::FileMetadata => summit_core::crypto::hash(b"summit.file.metadata"),
            Self::Message => summit_core::wire::messaging_hash(),
            Self::ComputeTask => summit_core::wire::compute_hash(),
            Self::Recovery => summit_core::wire::recovery_hash(),
        }
    }

    /// Human-readable name.
    pub fn name(&self) -> &'static str {
        match self {
            Self::TestPing => "summit.test.ping",
            Self::FileChunk => "summit.file.chunk",
            Self::FileData => "summit.file.data",
            Self::FileMetadata => "summit.file.metadata",
            Self::Message => "summit.messaging",
            Self::ComputeTask => "summit.compute",
            Self::Recovery => "summit.recovery",
        }
    }

    #[allow(clippy::type_complexity)]
    pub fn validator(&self) -> Option<Box<dyn Fn(&[u8]) -> bool + Send + Sync>> {
        match self {
            Self::TestPing => Some(Box::new(validate_test_ping)),
            Self::FileMetadata => Some(Box::new(validate_file_metadata)),
            Self::FileChunk | Self::FileData => None,
            Self::Message => None,
            Self::ComputeTask => None,
            Self::Recovery => None,
        }
    }
}

fn validate_test_ping(payload: &[u8]) -> bool {
    std::str::from_utf8(payload).is_ok()
}

fn validate_file_metadata(payload: &[u8]) -> bool {
    serde_json::from_slice::<FileMetadata>(payload).is_ok()
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
