//! Service trait for chunk-based services.
//!
//! Every Summit service processes chunks. This trait provides the
//! contract between the daemon (which receives/sends chunks) and
//! the service logic (which interprets them).

use anyhow::Result;
use summit_core::wire::{ChunkHeader, Contract, ServiceHash};

/// Trait for services that process incoming chunks and produce outgoing chunks.
///
/// Intentionally minimal. No request/response abstraction — that's an
/// application concern built on top of chunks.
pub trait ChunkService: Send + Sync {
    /// The service hash used in capability announcement.
    fn service_hash(&self) -> ServiceHash;

    /// The contract this service operates under.
    fn contract(&self) -> Contract;

    /// Called when this service is activated on a session with a peer.
    fn on_activate(&self, peer_pubkey: &[u8; 32]);

    /// Called when a session with a peer ends.
    fn on_deactivate(&self, peer_pubkey: &[u8; 32]);

    /// Handle an incoming chunk that belongs to this service.
    ///
    /// Called by the daemon's chunk dispatcher after decryption and
    /// hash verification. The payload is already validated.
    fn handle_chunk(
        &self,
        peer_pubkey: &[u8; 32],
        header: &ChunkHeader,
        payload: &[u8],
    ) -> Result<()>;
}
