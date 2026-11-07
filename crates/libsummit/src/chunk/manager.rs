//! Chunk manager — monitors the session table and spawns per-session
//! receive/handler tasks for newly established sessions.

use std::collections::HashSet;
use std::sync::Arc;
use std::time::Duration;

use tokio::sync::{broadcast, mpsc};

use summit_core::recovery::Capacity;
use summit_core::wire;
use summit_services::{
    ChunkCache, FileReassembler, OutgoingChunk, SendTarget, SessionTable, TrustLevel,
    TrustRegistry, UntrustedBuffer,
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
    outbound_tx: mpsc::Sender<(SendTarget, OutgoingChunk)>,
    shutdown: broadcast::Receiver<()>,
    bulk_rate: u32,
    bulk_burst: u32,
}

impl ChunkManager {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        sessions: SessionTable,
        cache: ChunkCache,
        delivery_tracker: DeliveryTracker,
        reassembler: Arc<FileReassembler>,
        trust: TrustRegistry,
        untrusted_buffer: UntrustedBuffer,
        dispatcher: Arc<ServiceDispatcher>,
        outbound_tx: mpsc::Sender<(SendTarget, OutgoingChunk)>,
        shutdown: broadcast::Receiver<()>,
        bulk_rate: u32,
        bulk_burst: u32,
    ) -> Self {
        Self {
            sessions,
            cache,
            delivery_tracker,
            reassembler,
            trust,
            untrusted_buffer,
            dispatcher,
            outbound_tx,
            shutdown,
            bulk_rate,
            bulk_burst,
        }
    }

    pub async fn run(mut self) -> anyhow::Result<()> {
        let mut interval = tokio::time::interval(Duration::from_secs(1));
        let seen_sessions = Arc::new(tokio::sync::Mutex::new(HashSet::new()));

        loop {
            tokio::select! {
                _ = self.shutdown.recv() => {
                    tracing::info!("chunk manager shutting down");
                    return Ok(());
                }

                _ = interval.tick() => {
                    self.spawn_new_sessions(&seen_sessions).await;
                }
            }
        }
    }

    async fn spawn_new_sessions(&self, seen_sessions: &Arc<tokio::sync::Mutex<HashSet<[u8; 32]>>>) {
        let mut seen = seen_sessions.lock().await;
        for entry in self.sessions.iter() {
            let session_id = *entry.key();
            if !seen.insert(session_id) {
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
            let bucket = active.bucket.clone();
            let reassembler = self.reassembler.clone();
            let peer_pubkey = active.meta.peer_pubkey;
            let service_hashes: Vec<_> = active.meta.active_services.keys().copied().collect();
            let trust = self.trust.clone();
            let buffer = self.untrusted_buffer.clone();
            let dispatcher = self.dispatcher.clone();
            let cache = self.cache.clone();
            let tracker = self.delivery_tracker.clone();
            let outbound_tx = self.outbound_tx.clone();

            // Notify services that this peer's session is now active.
            dispatcher.activate_session(&peer_pubkey, &service_hashes);

            // Send our bulk capacity to the peer
            let capacity = Capacity {
                bulk_rate: self.bulk_rate,
                bulk_burst: self.bulk_burst,
            };
            if let Ok(payload) = serde_json::to_vec(&capacity) {
                let cap_chunk = OutgoingChunk {
                    type_tag: wire::recovery::CAPACITY,
                    schema_id: wire::recovery_hash(),
                    payload: bytes::Bytes::from(payload),
                    priority_flags: 0x01, // Realtime — bypasses token bucket
                };
                let cap_tx = self.outbound_tx.clone();
                let _ = cap_tx
                    .send((
                        SendTarget::Peer {
                            public_key: peer_pubkey,
                        },
                        cap_chunk,
                    ))
                    .await;
            }

            // Create channel for received chunks
            let (chunk_tx, mut chunk_rx) = tokio::sync::mpsc::channel::<super::IncomingChunk>(100);

            let reassembler_for_handler = reassembler.clone();

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
                            buffer.add(
                                peer_pubkey,
                                chunk.content_hash,
                                chunk.type_tag,
                                chunk.schema_id,
                                chunk.payload,
                            );
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
                            if let Err(e) = reassembler_for_handler
                                .add_metadata(chunk.content_hash, metadata, peer_pubkey)
                                .await
                            {
                                tracing::warn!(error = %e, "file metadata handling failed");
                            }
                        }
                    }

                    // Handle file data chunks (type_tag 2)
                    if chunk.type_tag == 2 {
                        if let Ok(paths) = reassembler_for_handler
                            .add_chunk(chunk.content_hash, chunk.payload)
                            .await
                        {
                            for path in paths {
                                tracing::info!(path = %path.display(), "file completed");
                            }
                        }
                    }
                }
            });

            // Spawn receive loop — removes session from table on exit
            let peer_addr_str = peer_addr.to_string();
            let session_table = self.sessions.clone();
            let seen = seen_sessions.clone();
            let deactivate_dispatcher = self.dispatcher.clone();
            let deactivate_hashes = service_hashes.clone();
            let recv_trust = self.trust.clone();
            let recv_untrusted = self.untrusted_buffer.clone();
            tokio::spawn(async move {
                let receive = super::receive::receive_loop(
                    socket,
                    crypto,
                    chunk_tx,
                    outbound_tx,
                    cache,
                    tracker,
                    peer_addr_str,
                    dispatcher,
                    peer_pubkey,
                    bucket,
                    reassembler,
                    recv_trust,
                    recv_untrusted,
                );
                // Stop as soon as the session leaves the table (superseded by
                // a new handshake, or dropped via the API), not only on the
                // receive timeout.
                tokio::select! {
                    r = receive => if let Err(e) = r {
                        tracing::warn!(error = %e, "receive loop terminated");
                    },
                    _ = removed_from(&session_table, session_id) => {
                        tracing::info!(
                            session_id = hex::encode(session_id),
                            "session removed, stopping its receive loop"
                        );
                    }
                }
                // Notify services that this peer's session has ended, unless
                // a newer session with the same peer is already up.
                let replaced = session_table
                    .iter()
                    .any(|s| s.value().meta.peer_pubkey == peer_pubkey && *s.key() != session_id);
                if !replaced {
                    deactivate_dispatcher.deactivate_session(&peer_pubkey, &deactivate_hashes);
                }
                // Prune the dead session so the initiator can reconnect
                if session_table.remove(&session_id).is_some() {
                    tracing::info!(
                        session_id = hex::encode(session_id),
                        "pruned dead session from table"
                    );
                }
                seen.lock().await.remove(&session_id);
            });
        }
    }
}

/// Resolves once `session_id` is no longer in the session table.
async fn removed_from(table: &SessionTable, session_id: [u8; 32]) {
    let mut tick = tokio::time::interval(Duration::from_millis(500));
    loop {
        tick.tick().await;
        if !table.contains_key(&session_id) {
            return;
        }
    }
}
