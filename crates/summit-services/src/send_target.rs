//! Send targeting — broadcast vs peer vs session targeting.

use serde::{Deserialize, Serialize};

/// Target for chunk sending.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum SendTarget {
    /// Broadcast to all trusted sessions.
    #[default]
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn serde_roundtrip_broadcast() {
        let target = SendTarget::Broadcast;
        let json = serde_json::to_string(&target).unwrap();
        let back: SendTarget = serde_json::from_str(&json).unwrap();
        assert!(matches!(back, SendTarget::Broadcast));
    }

    #[test]
    fn serde_roundtrip_peer() {
        let key = [0xabu8; 32];
        let target = SendTarget::Peer { public_key: key };
        let json = serde_json::to_string(&target).unwrap();
        let back: SendTarget = serde_json::from_str(&json).unwrap();
        match back {
            SendTarget::Peer { public_key } => assert_eq!(public_key, key),
            _ => panic!("expected Peer variant"),
        }
    }

    #[test]
    fn serde_roundtrip_session() {
        let id = [0xcdu8; 32];
        let target = SendTarget::Session { session_id: id };
        let json = serde_json::to_string(&target).unwrap();
        let back: SendTarget = serde_json::from_str(&json).unwrap();
        match back {
            SendTarget::Session { session_id } => assert_eq!(session_id, id),
            _ => panic!("expected Session variant"),
        }
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
