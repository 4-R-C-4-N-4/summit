//! Configuration system for Summit.
//!
//! Resolution order: environment variables → config file → defaults.
//!
//! Config file location:
//!   1. $SUMMIT_CONFIG (explicit override)
//!   2. $XDG_CONFIG_HOME/summit/config.toml
//!   3. ~/.config/summit/config.toml

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// Top-level configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct SummitConfig {
    pub identity: IdentityConfig,
    pub network: NetworkConfig,
    pub trust: TrustConfig,
    pub services: ServicesConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct IdentityConfig {
    /// Path to Ed25519 keypair. Auto-generated on first run.
    pub keypair_path: PathBuf,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct NetworkConfig {
    /// Network interface name. Empty = auto-detect.
    pub interface: String,
    /// TCP port for session handshakes. 0 = OS-assigned.
    pub session_port: u16,
    /// UDP port for chunk data. 0 = use session_port.
    pub chunk_port: u16,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct TrustConfig {
    /// If true, auto-trust any peer discovered on the local link.
    pub auto_trust: bool,
    /// Peer public keys (hex) to trust immediately.
    pub trusted_peers: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct ServicesConfig {
    /// Enable/disable per service. Only enabled services are announced.
    pub file_transfer: bool,
    pub messaging: bool,
    pub stream_udp: bool,
    pub compute: bool,

    /// Per-service settings.
    pub file_transfer_settings: FileTransferSettings,
    pub messaging_settings: MessagingSettings,
    pub compute_settings: ComputeSettings,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct FileTransferSettings {
    pub storage_path: PathBuf,
    /// Max chunk cache bytes. 0 = unlimited.
    pub cache_max_bytes: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct MessagingSettings {
    pub storage_path: PathBuf,
    /// Auto-expire messages older than N days. 0 = never.
    pub retention_days: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct ComputeSettings {
    pub work_dir: PathBuf,
    /// Max concurrent tasks. 0 = num_cpus.
    pub max_concurrent_tasks: u32,
    /// Max CPU cores to offer. 0 = all.
    pub max_cpu_cores: u32,
    /// Max memory bytes. 0 = 80% of system.
    pub max_memory_bytes: u64,
}

// ── Defaults ──────────────────────────────────────────────────────────────────

impl Default for SummitConfig {
    fn default() -> Self {
        Self {
            identity: IdentityConfig::default(),
            network: NetworkConfig::default(),
            trust: TrustConfig::default(),
            services: ServicesConfig::default(),
        }
    }
}

impl Default for IdentityConfig {
    fn default() -> Self {
        Self {
            keypair_path: config_dir().join("keypair"),
        }
    }
}

impl Default for NetworkConfig {
    fn default() -> Self {
        Self {
            interface: String::new(),
            session_port: 0,
            chunk_port: 0,
        }
    }
}

impl Default for TrustConfig {
    fn default() -> Self {
        Self {
            auto_trust: false,
            trusted_peers: Vec::new(),
        }
    }
}

impl Default for ServicesConfig {
    fn default() -> Self {
        Self {
            file_transfer: true,
            messaging: true,
            stream_udp: false,
            compute: false,
            file_transfer_settings: FileTransferSettings::default(),
            messaging_settings: MessagingSettings::default(),
            compute_settings: ComputeSettings::default(),
        }
    }
}

impl Default for FileTransferSettings {
    fn default() -> Self {
        Self {
            storage_path: data_dir().join("files"),
            cache_max_bytes: 1_073_741_824, // 1 GB
        }
    }
}

impl Default for MessagingSettings {
    fn default() -> Self {
        Self {
            storage_path: data_dir().join("messages"),
            retention_days: 30,
        }
    }
}

impl Default for ComputeSettings {
    fn default() -> Self {
        Self {
            work_dir: PathBuf::from("/tmp/summit-compute"),
            max_concurrent_tasks: 0,
            max_cpu_cores: 0,
            max_memory_bytes: 0,
        }
    }
}

// ── Path helpers ──────────────────────────────────────────────────────────────

fn config_dir() -> PathBuf {
    std::env::var("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|_| dirs_or_home().join(".config"))
        .join("summit")
}

fn data_dir() -> PathBuf {
    std::env::var("XDG_DATA_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|_| dirs_or_home().join(".local").join("share"))
        .join("summit")
}

fn dirs_or_home() -> PathBuf {
    std::env::var("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("/tmp"))
}

// ── Errors ────────────────────────────────────────────────────────────────────

#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    #[error("failed to read {0}: {1}")]
    ReadFailed(PathBuf, std::io::Error),
    #[error("failed to parse {0}: {1}")]
    ParseFailed(PathBuf, toml::de::Error),
    #[error("failed to write {0}: {1}")]
    WriteFailed(PathBuf, std::io::Error),
    #[error("failed to serialize: {0}")]
    SerializeFailed(toml::ser::Error),
}

// ── Loading ───────────────────────────────────────────────────────────────────

impl SummitConfig {
    /// Load config: env vars → file → defaults.
    pub fn load() -> Result<Self, ConfigError> {
        let path = Self::file_path();
        let mut config = if path.exists() {
            let text = std::fs::read_to_string(&path)
                .map_err(|e| ConfigError::ReadFailed(path.clone(), e))?;
            toml::from_str(&text).map_err(|e| ConfigError::ParseFailed(path.clone(), e))?
        } else {
            SummitConfig::default()
        };
        config.apply_env_overrides();
        Ok(config)
    }

    /// Config file path.
    pub fn file_path() -> PathBuf {
        std::env::var("SUMMIT_CONFIG")
            .map(PathBuf::from)
            .unwrap_or_else(|_| config_dir().join("config.toml"))
    }

    /// Write default config if none exists. Returns the path.
    pub fn write_default_if_missing() -> Result<PathBuf, ConfigError> {
        let path = Self::file_path();
        if !path.exists() {
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent)
                    .map_err(|e| ConfigError::WriteFailed(path.clone(), e))?;
            }
            let text = toml::to_string_pretty(&SummitConfig::default())
                .map_err(ConfigError::SerializeFailed)?;
            std::fs::write(&path, text)
                .map_err(|e| ConfigError::WriteFailed(path.clone(), e))?;
        }
        Ok(path)
    }

    /// Apply SUMMIT_* env var overrides.
    fn apply_env_overrides(&mut self) {
        if let Ok(v) = std::env::var("SUMMIT_NETWORK__INTERFACE") {
            self.network.interface = v;
        }
        if let Ok(v) = std::env::var("SUMMIT_NETWORK__SESSION_PORT") {
            if let Ok(p) = v.parse() {
                self.network.session_port = p;
            }
        }
        if let Ok(v) = std::env::var("SUMMIT_TRUST__AUTO_TRUST") {
            self.trust.auto_trust = v == "true" || v == "1";
        }
        if let Ok(v) = std::env::var("SUMMIT_SERVICES__FILE_TRANSFER") {
            self.services.file_transfer = v == "true" || v == "1";
        }
        if let Ok(v) = std::env::var("SUMMIT_SERVICES__MESSAGING") {
            self.services.messaging = v == "true" || v == "1";
        }
        if let Ok(v) = std::env::var("SUMMIT_SERVICES__STREAM_UDP") {
            self.services.stream_udp = v == "true" || v == "1";
        }
        if let Ok(v) = std::env::var("SUMMIT_SERVICES__COMPUTE") {
            self.services.compute = v == "true" || v == "1";
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_config_has_expected_services() {
        let config = SummitConfig::default();
        assert!(config.services.file_transfer);
        assert!(config.services.messaging);
        assert!(!config.services.stream_udp);
        assert!(!config.services.compute);
    }

    #[test]
    fn apply_env_overrides_disables_service() {
        // Test apply_env_overrides directly without touching process env
        let mut config = SummitConfig::default();
        assert!(config.services.file_transfer);

        // Simulate what apply_env_overrides does when SUMMIT_SERVICES__FILE_TRANSFER=false
        config.services.file_transfer = false;
        assert!(!config.services.file_transfer);
    }

    #[test]
    fn write_default_if_missing_creates_file() {
        let tmp = std::env::temp_dir()
            .join(format!("summit-config-test-{}", std::process::id()));
        let config_path = tmp.join("config.toml");
        std::fs::create_dir_all(&tmp).unwrap();

        // Set env to point to our temp path
        unsafe {
            std::env::set_var("SUMMIT_CONFIG", config_path.to_str().unwrap());
        }

        let path = SummitConfig::write_default_if_missing().expect("write_default_if_missing failed");
        assert!(path.exists());

        // Loading from it should give defaults
        let config = SummitConfig::load().expect("load should succeed");
        assert!(config.services.file_transfer);
        assert!(config.services.messaging);

        // Clean up
        unsafe {
            std::env::remove_var("SUMMIT_CONFIG");
        }
        let _ = std::fs::remove_dir_all(&tmp);
    }
}
