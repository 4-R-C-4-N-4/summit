//! Chunk manager — monitors the session table and spawns per-session
//! receive/handler tasks for newly established sessions.

use std::collections::HashSet;
use std::sync::Arc;
use std::time::Duration;

use tokio::sync::broadcast;

use summit_services::{
    ChunkCache, FileReassembler, SessionTable, TrustLevel, TrustRegistry, UntrustedBuffer,
};

use crate::delivery::DeliveryTracker;
use crate::dispatch::ServiceDispatcher;

pub struct ChunkManager {
    sessions: SessionTable,
    cache: ChunkCache,
    delivery_tracker: DeliveryTracker,
    reassembler: Arc<FileReassembler>,
    trust: TrustRegistry,
    untrusted_buffer: UntrustedBuffer,
    dispatcher: Arc<ServiceDispatcher>,
    shutdown: broadcast::Receiver<()>,
}

impl ChunkManager {
    pub fn new(
        sessions: SessionTable,
        cache: ChunkCache,
        delivery_tracker: DeliveryTracker,
        reassembler: Arc<FileReassembler>,
        trust: TrustRegistry,
        untrusted_buffer: UntrustedBuffer,
        dispatcher: Arc<ServiceDispatcher>,
        shutdown: broadcast::Receiver<()>,
    ) -> Self {
        Self {
            sessions,
            cache,
            delivery_tracker,
            reassembler,
            trust,
            untrusted_buffer,
            dispatcher,
            shutdown,
        }
    }

    pub async fn run(mut self) -> anyhow::Result<()> {
        let mut interval = tokio::time::interval(Duration::from_secs(1));
        let mut seen_sessions = HashSet::new();

        loop {
            tokio::select! {
                _ = self.shutdown.recv() => {
                    tracing::info!("chunk manager shutting down");
                    return Ok(());
                }

                _ = interval.tick() => {
                    self.spawn_new_sessions(&mut seen_sessions);
                }
            }
        }
    }

    fn spawn_new_sessions(&self, seen_sessions: &mut HashSet<[u8; 32]>) {
        for entry in self.sessions.iter() {
            let session_id = *entry.key();
            if !seen_sessions.insert(session_id) {
                continue;
            }

            let active = entry.value();
            tracing::info!(
                session_id = hex::encode(session_id),
                "spawning chunk tasks for session"
            );

            let peer_addr = active.meta.peer_addr;
            let crypto = active.crypto.clone();
            let socket = active.socket.clone();
            let reassembler = self.reassembler.clone();
            let peer_pubkey = active.meta.peer_pubkey;
            let trust = self.trust.clone();
            let buffer = self.untrusted_buffer.clone();
            let dispatcher = self.dispatcher.clone();
            let cache = self.cache.clone();
            let tracker = self.delivery_tracker.clone();

            // Create channel for received chunks
            let (chunk_tx, mut chunk_rx) = tokio::sync::mpsc::channel::<super::IncomingChunk>(100);

            // Spawn receiver handler (processes chunks, feeds reassembler)
            tokio::spawn(async move {
                while let Some(chunk) = chunk_rx.recv().await {
                    // Check trust level BEFORE processing
                    match trust.check(&peer_pubkey) {
                        TrustLevel::Blocked => {
                            tracing::debug!(
                                peer = hex::encode(peer_pubkey),
                                "chunk from blocked peer, dropping"
                            );
                            continue;
                        }
                        TrustLevel::Untrusted => {
                            tracing::info!(
                                peer = hex::encode(&peer_pubkey[..8]),
                                content_hash = hex::encode(&chunk.content_hash[..8]),
                                "chunk from untrusted peer, buffering"
                            );
                            buffer.add(peer_pubkey, chunk.content_hash, chunk.payload);
                            continue;
                        }
                        TrustLevel::Trusted => {
                            // Process normally
                        }
                    }

                    tracing::info!(
                        content_hash = hex::encode(chunk.content_hash),
                        type_tag = chunk.type_tag,
                        payload_len = chunk.payload.len(),
                        "chunk received"
                    );

                    // Handle file metadata chunks (type_tag 3)
                    if chunk.type_tag == 3 {
                        if let Ok(metadata) =
                            serde_json::from_slice::<summit_services::FileMetadata>(&chunk.payload)
                        {
                            tracing::info!(filename = %metadata.filename, chunks = metadata.chunk_hashes.len(), "file transfer started");
                            reassembler.add_metadata(metadata).await;
                        }
                    }

                    // Handle file data chunks (type_tag 2)
                    if chunk.type_tag == 2 {
                        if let Ok(Some(path)) = reassembler
                            .add_chunk(chunk.content_hash, chunk.payload)
                            .await
                        {
                            tracing::info!(path = %path.display(), "file completed");
                        }
                    }
                }
            });

            // Spawn receive loop
            let peer_addr_str = peer_addr.to_string();
            tokio::spawn(async move {
                if let Err(e) = super::receive::receive_loop(
                    socket,
                    crypto,
                    chunk_tx,
                    cache,
                    tracker,
                    peer_addr_str,
                    dispatcher,
                    peer_pubkey,
                )
                .await
                {
                    tracing::warn!(error = %e, "receive loop terminated");
                }
            });
        }
    }
}
