//! Send worker — dequeues outbound chunks, resolves targets,
//! applies QoS, and sends to appropriate sessions.

use tokio::sync::{broadcast, mpsc};

use summit_core::wire::Contract;
use summit_services::{ChunkCache, SendTarget, SessionTable, TrustLevel, TrustRegistry};

use super::OutgoingChunk;

pub struct SendWorker {
    sessions: SessionTable,
    cache: ChunkCache,
    trust: TrustRegistry,
    chunk_rx: mpsc::Receiver<(SendTarget, OutgoingChunk)>,
    shutdown: broadcast::Receiver<()>,
}

impl SendWorker {
    pub fn new(
        sessions: SessionTable,
        cache: ChunkCache,
        trust: TrustRegistry,
        chunk_rx: mpsc::Receiver<(SendTarget, OutgoingChunk)>,
        shutdown: broadcast::Receiver<()>,
    ) -> Self {
        Self {
            sessions,
            cache,
            trust,
            chunk_rx,
            shutdown,
        }
    }

    pub async fn run(mut self) -> anyhow::Result<()> {
        loop {
            tokio::select! {
                _ = self.shutdown.recv() => {
                    tracing::info!("send worker shutting down");
                    return Ok(());
                }

                msg = self.chunk_rx.recv() => {
                    let (target, chunk) = match msg {
                        Some(m) => m,
                        None => {
                            tracing::info!("chunk_tx dropped, send worker exiting");
                            return Ok(());
                        }
                    };
                    self.send_to_targets(target, chunk).await;
                }
            }
        }
    }

    async fn send_to_targets(&self, target: SendTarget, chunk: OutgoingChunk) {
        // Determine which sessions to send to based on target
        let target_sessions: Vec<[u8; 32]> = match &target {
            SendTarget::Broadcast => self
                .sessions
                .iter()
                .filter(|e| self.trust.check(&e.value().meta.peer_pubkey) == TrustLevel::Trusted)
                .map(|e| *e.key())
                .collect(),
            SendTarget::Peer { public_key } => self
                .sessions
                .iter()
                .find(|e| e.value().meta.peer_pubkey == *public_key)
                .map(|e| vec![*e.key()])
                .unwrap_or_default(),
            SendTarget::Session { session_id } => {
                if self.sessions.contains_key(session_id) {
                    vec![*session_id]
                } else {
                    vec![]
                }
            }
        };

        if target_sessions.is_empty() {
            tracing::debug!(?target, "no target sessions found");
            return;
        }

        // Priority check
        let has_realtime = self
            .sessions
            .iter()
            .any(|e| matches!(e.value().meta.primary_contract(), Contract::Realtime));

        let mut send_tasks = Vec::new();

        for session_id in target_sessions {
            let session = match self.sessions.get(&session_id) {
                Some(s) => s,
                None => continue,
            };
            let peer_addr = session.meta.peer_addr;
            let chunk_port = session.meta.chunk_port;
            let socket = session.value().socket.clone();
            let crypto = session.value().crypto.clone();
            let contract = session.meta.primary_contract();

            // Drop Background if Realtime is active
            if has_realtime && matches!(contract, Contract::Background) {
                tracing::debug!(%peer_addr, "background chunk suppressed — realtime active");
                continue;
            }

            // Realtime-priority chunks bypass the token bucket entirely.
            // This includes NACK retransmissions and recovery protocol messages.
            if chunk.priority_flags != 0x01 {
                let allowed = session.bucket.lock().await.allow();
                if !allowed {
                    tracing::debug!(%peer_addr, ?contract, "chunk dropped — rate limited");
                    continue;
                }
            }

            // Construct chunk peer address
            let chunk_peer_addr = match peer_addr {
                std::net::SocketAddr::V6(mut addr) => {
                    addr.set_port(chunk_port);
                    std::net::SocketAddr::V6(addr)
                }
                _ => peer_addr,
            };

            let chunk_clone = chunk.clone();
            let cache_clone = self.cache.clone();

            let task = tokio::spawn(async move {
                super::send::send_chunk(socket, chunk_peer_addr, crypto, chunk_clone, cache_clone)
                    .await
            });
            send_tasks.push(task);
        }

        // Wait for all sends to complete
        for task in send_tasks {
            let _ = task.await;
        }
    }
}
