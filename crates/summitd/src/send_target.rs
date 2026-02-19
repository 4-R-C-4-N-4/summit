//! Send targeting — broadcast vs peer vs session targeting.

use serde::{Deserialize, Serialize};

/// Target for chunk sending.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum SendTarget {
    /// Broadcast to all trusted sessions.
    Broadcast,

    /// Send to specific peer by public key.
    #[serde(rename = "peer")]
    Peer {
        #[serde(with = "hex_serde")]
        public_key: [u8; 32],
    },

    /// Send to specific session by session ID.
    #[serde(rename = "session")]
    Session {
        #[serde(with = "hex_serde")]
        session_id: [u8; 32],
    },
}

impl Default for SendTarget {
    fn default() -> Self {
        Self::Broadcast
    }
}

mod hex_serde {
    use serde::{Deserialize, Deserializer, Serializer};

    pub fn serialize<S>(bytes: &[u8; 32], serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&hex::encode(bytes))
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<[u8; 32], D::Error>
    where
        D: Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        let bytes = hex::decode(&s).map_err(serde::de::Error::custom)?;
        if bytes.len() != 32 {
            return Err(serde::de::Error::custom("expected 32 bytes"));
        }
        let mut arr = [0u8; 32];
        arr.copy_from_slice(&bytes);
        Ok(arr)
    }
}
