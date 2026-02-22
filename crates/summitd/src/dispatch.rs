//! Routes incoming chunks to the appropriate service based on schema_id.

use std::collections::HashMap;
use std::sync::Arc;

use summit_core::wire::{ChunkHeader, ServiceHash};
use summit_services::ChunkService;

/// Maps schema_ids to services and dispatches incoming chunks.
pub struct ServiceDispatcher {
    /// schema_id -> service. Multiple schema_ids can map to one service.
    schema_to_service: HashMap<[u8; 32], Arc<dyn ChunkService>>,
    /// All registered services by service hash (for activate/deactivate).
    services: HashMap<ServiceHash, Arc<dyn ChunkService>>,
}

impl ServiceDispatcher {
    pub fn new() -> Self {
        Self {
            schema_to_service: HashMap::new(),
            services: HashMap::new(),
        }
    }

    /// Register a service. Also registers service_hash as the default schema mapping.
    pub fn register(&mut self, service: Arc<dyn ChunkService>) {
        let hash = service.service_hash();
        // Default: the service hash IS the schema ID.
        self.schema_to_service.insert(hash, service.clone());
        self.services.insert(hash, service);
    }

    /// Register an additional schema_id -> service mapping.
    /// Use when a service handles multiple schema types.
    pub fn register_schema(&mut self, schema_id: [u8; 32], service: Arc<dyn ChunkService>) {
        self.schema_to_service.insert(schema_id, service);
    }

    /// Dispatch an incoming chunk to the appropriate service.
    /// Returns false if no service handles this schema_id.
    pub fn dispatch(
        &self,
        peer_pubkey: &[u8; 32],
        header: &ChunkHeader,
        payload: &[u8],
    ) -> bool {
        if let Some(service) = self.schema_to_service.get(&header.schema_id) {
            if let Err(e) = service.handle_chunk(peer_pubkey, header, payload) {
                tracing::warn!(
                    schema_id = hex::encode(header.schema_id),
                    error = %e,
                    "service chunk handling failed"
                );
            }
            true
        } else {
            false
        }
    }

    /// Notify services when a session is established.
    pub fn activate_session(&self, peer_pubkey: &[u8; 32], active_service_hashes: &[ServiceHash]) {
        for hash in active_service_hashes {
            if let Some(service) = self.services.get(hash) {
                service.on_activate(peer_pubkey);
            }
        }
    }

    /// Notify services when a session ends.
    pub fn deactivate_session(
        &self,
        peer_pubkey: &[u8; 32],
        active_service_hashes: &[ServiceHash],
    ) {
        for hash in active_service_hashes {
            if let Some(service) = self.services.get(hash) {
                service.on_deactivate(peer_pubkey);
            }
        }
    }
}
