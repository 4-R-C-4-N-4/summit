//! Chunk manager — monitors the session table and spawns per-session
//! receive/handler tasks for newly established sessions.

use std::collections::HashSet;
use std::sync::Arc;
use std::time::Duration;

use tokio::sync::{broadcast, mpsc};

use summit_core::recovery::{Capacity, Gone};
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
            let trust = self.trust.clone();
            let buffer = self.untrusted_buffer.clone();
            let dispatcher = self.dispatcher.clone();
            let cache = self.cache.clone();
            let tracker = self.delivery_tracker.clone();
            let outbound_tx = self.outbound_tx.clone();

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

                    // Handle GONE — sender can't provide these chunks
                    if chunk.schema_id == wire::recovery_hash()
                        && chunk.type_tag == wire::recovery::GONE
                    {
                        if let Ok(gone) = serde_json::from_slice::<Gone>(&chunk.payload) {
                            let stalled = reassembler.missing_chunks().await;
                            for (filename, missing) in stalled {
                                let any_gone = missing.iter().any(|h| gone.hashes.contains(h));
                                if any_gone {
                                    reassembler.abandon(&filename).await;
                                }
                            }
                        }
                        continue;
                    }

                    // Handle file metadata chunks (type_tag 3)
                    if chunk.type_tag == 3 {
                        if let Ok(metadata) =
                            serde_json::from_slice::<summit_services::FileMetadata>(&chunk.payload)
                        {
                            tracing::info!(filename = %metadata.filename, chunks = metadata.chunk_hashes.len(), "file transfer started");
                            reassembler.add_metadata(metadata, peer_pubkey).await;
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

            // Spawn receive loop — removes session from table on exit
            let peer_addr_str = peer_addr.to_string();
            let session_table = self.sessions.clone();
            let seen = seen_sessions.clone();
            tokio::spawn(async move {
                if let Err(e) = super::receive::receive_loop(
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
                )
                .await
                {
                    tracing::warn!(error = %e, "receive loop terminated");
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
