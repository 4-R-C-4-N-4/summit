//! Inbound session handshake listener.
//!
//! Handles Noise_XX Init → Response → Complete and the subsequent
//! encrypted chunk_port exchange that finalises session setup.

use std::net::{Ipv6Addr, SocketAddr};
use std::sync::Arc;
use std::time::Duration;

use anyhow::Result;
use tokio::net::UdpSocket;
use tokio::sync::{broadcast, Mutex};
use zerocopy::{AsBytes, FromBytes};

use summit_core::crypto::{Keypair, NoiseResponder};
use summit_core::wire::{Contract, HandshakeComplete, HandshakeInit, HandshakeResponse};
use summit_services::{ActiveSession, PeerRegistry, SessionMeta, SessionTable, TokenBucket};

use super::default_active_services;
use super::state::SharedTracker;

pub struct SessionListener {
    socket: Arc<UdpSocket>,
    keypair: Arc<Keypair>,
    sessions: SessionTable,
    tracker: SharedTracker,
    local_addr: Ipv6Addr,
    registry: PeerRegistry,
    shutdown: broadcast::Receiver<()>,
}

impl SessionListener {
    pub fn new(
        socket: Arc<UdpSocket>,
        keypair: Arc<Keypair>,
        sessions: SessionTable,
        tracker: SharedTracker,
        local_addr: Ipv6Addr,
        registry: PeerRegistry,
        shutdown: broadcast::Receiver<()>,
    ) -> Self {
        Self {
            socket,
            keypair,
            sessions,
            tracker,
            local_addr,
            registry,
            shutdown,
        }
    }

    pub async fn run(mut self) -> Result<()> {
        const HANDSHAKE_INIT_SIZE: usize = std::mem::size_of::<HandshakeInit>();
        const HANDSHAKE_RESPONSE_SIZE: usize = std::mem::size_of::<HandshakeResponse>();
        const HANDSHAKE_COMPLETE_SIZE: usize = std::mem::size_of::<HandshakeComplete>();

        let mut buf = vec![0u8; 512];
        let mut cleanup_interval = tokio::time::interval(Duration::from_secs(5));

        loop {
            tokio::select! {
                _ = self.shutdown.recv() => {
                    tracing::info!("session listener shutting down");
                    return Ok(());
                }

                _ = cleanup_interval.tick() => {
                    self.tracker.lock().await.cleanup_stale();
                }

                result = self.socket.recv_from(&mut buf) => {
                    let (len, peer_addr) = match result {
                        Ok(r) => r,
                        Err(e) => {
                            tracing::warn!(error = %e, "recv_from failed");
                            continue;
                        }
                    };

                    let peer_ip = match peer_addr {
                        SocketAddr::V6(v6) => *v6.ip(),
                        _ => { tracing::warn!("ignoring IPv4 peer"); continue; }
                    };

                    if peer_ip == self.local_addr {
                        tracing::trace!("ignoring loopback from own IP");
                        continue;
                    }

                    let data = &buf[..len];

                    if len == HANDSHAKE_INIT_SIZE {
                        self.handle_init(data, peer_addr, peer_ip).await;
                    } else if len == HANDSHAKE_RESPONSE_SIZE {
                        self.handle_response(data, peer_addr, peer_ip).await;
                    } else if len == HANDSHAKE_COMPLETE_SIZE {
                        self.handle_complete(data, peer_addr, peer_ip).await;
                    } else {
                        self.handle_chunk_port_exchange(data, peer_addr, peer_ip).await;
                    }
                }
            }
        }
    }

    async fn handle_init(&self, data: &[u8], peer_addr: SocketAddr, peer_ip: Ipv6Addr) {
        let init = match HandshakeInit::read_from(data) {
            Some(m) => m,
            None => {
                tracing::warn!("failed to parse HandshakeInit");
                return;
            }
        };

        tracing::debug!(peer_addr = %peer_addr, "received HandshakeInit");

        // Deduplicate
        {
            let t = self.tracker.lock().await;
            if t.has_initiator(&peer_ip) || t.has_initiator_waiting(&peer_ip) {
                tracing::debug!(%peer_addr, "already initiating to this peer, ignoring HandshakeInit");
                return;
            }
            if t.has_responder(&peer_ip) || t.has_responder_waiting(&peer_ip) {
                tracing::debug!(%peer_addr, "duplicate HandshakeInit, ignoring");
                return;
            }
        }

        // Create chunk socket
        let chunk_socket = match UdpSocket::bind("[::]:0").await {
            Ok(s) => Arc::new(s),
            Err(e) => {
                tracing::warn!(error = %e, "failed to bind chunk socket");
                return;
            }
        };
        let local_chunk_port = match chunk_socket.local_addr() {
            Ok(addr) => addr.port(),
            Err(e) => {
                tracing::warn!(error = %e, "failed to get chunk port");
                return;
            }
        };

        // Create noise responder
        let noise = match NoiseResponder::new(&self.keypair) {
            Ok(n) => n,
            Err(e) => {
                tracing::warn!(error = %e, "failed to create noise responder");
                return;
            }
        };
        let responder_nonce = *noise.nonce();

        // Process Noise message 1
        let (pending, msg2) = match noise.respond(&init.noise_msg, &init.nonce) {
            Ok(r) => r,
            Err(e) => {
                tracing::warn!(error = %e, "noise.respond failed");
                return;
            }
        };

        // Send HandshakeResponse
        let response = HandshakeResponse {
            nonce: responder_nonce,
            noise_msg: match msg2.try_into() {
                Ok(m) => m,
                Err(_) => {
                    tracing::warn!("msg2 wrong size");
                    return;
                }
            },
        };
        if let Err(e) = self.socket.send_to(response.as_bytes(), peer_addr).await {
            tracing::warn!(error = %e, "failed to send HandshakeResponse");
            return;
        }
        tracing::debug!(peer_addr = %peer_addr, "sent HandshakeResponse");

        // Look up peer's public key from registry
        let peer_pubkey = match self
            .registry
            .iter()
            .find(|entry| entry.value().addr == peer_ip)
            .map(|entry| *entry.key())
        {
            Some(pk) => pk,
            None => {
                tracing::warn!(
                    %peer_addr,
                    "HandshakeInit from peer not yet in registry, deferring"
                );
                return;
            }
        };

        self.tracker.lock().await.add_responder(
            peer_ip,
            peer_pubkey,
            pending,
            local_chunk_port,
            chunk_socket,
        );
    }

    async fn handle_response(&self, data: &[u8], peer_addr: SocketAddr, peer_ip: Ipv6Addr) {
        let response = match HandshakeResponse::read_from(data) {
            Some(m) => m,
            None => {
                tracing::warn!("failed to parse HandshakeResponse");
                return;
            }
        };

        tracing::debug!(peer_addr = %peer_addr, "received HandshakeResponse");

        // Deduplicate
        {
            let t = self.tracker.lock().await;
            if t.has_initiator_waiting(&peer_ip) {
                tracing::debug!(%peer_addr, "duplicate HandshakeResponse, ignoring");
                return;
            }
        }

        let state = match self.tracker.lock().await.remove_initiator(&peer_ip) {
            Some(s) => s,
            None => {
                tracing::warn!(%peer_addr, "HandshakeResponse for unknown handshake");
                return;
            }
        };

        // Finish Noise handshake
        let (mut session, msg3) = match state.noise.finish(&response.noise_msg, &response.nonce) {
            Ok(r) => r,
            Err(e) => {
                tracing::warn!(error = %e, "noise.finish failed");
                return;
            }
        };

        // Send HandshakeComplete
        let complete = HandshakeComplete {
            noise_msg: match msg3.try_into() {
                Ok(m) => m,
                Err(_) => {
                    tracing::warn!("msg3 wrong size");
                    return;
                }
            },
        };
        if let Err(e) = self.socket.send_to(complete.as_bytes(), peer_addr).await {
            tracing::warn!(error = %e, "failed to send HandshakeComplete");
            return;
        }
        tracing::debug!(peer_addr = %peer_addr, "sent HandshakeComplete");

        // Send our chunk_port encrypted
        let chunk_port_msg = state.chunk_socket_port.to_le_bytes();
        let mut encrypted = Vec::new();
        if let Err(e) = session.encrypt(&chunk_port_msg, &mut encrypted) {
            tracing::warn!(error = %e, "failed to encrypt chunk_port");
            return;
        }
        if let Err(e) = self.socket.send_to(&encrypted, peer_addr).await {
            tracing::warn!(error = %e, "failed to send chunk_port");
            return;
        }
        tracing::debug!(peer_addr = %peer_addr, "sent chunk_port (initiator)");

        self.tracker.lock().await.add_initiator_waiting_chunk(
            peer_ip,
            session,
            state.chunk_socket,
            state.chunk_socket_port,
            state.peer_pubkey,
        );
    }

    async fn handle_complete(&self, data: &[u8], peer_addr: SocketAddr, peer_ip: Ipv6Addr) {
        let complete = match HandshakeComplete::read_from(data) {
            Some(m) => m,
            None => {
                tracing::warn!("failed to parse HandshakeComplete");
                return;
            }
        };

        tracing::debug!(peer_addr = %peer_addr, "received HandshakeComplete");

        let state = match self.tracker.lock().await.remove_responder(&peer_ip) {
            Some(s) => s,
            None => {
                tracing::warn!(%peer_addr, "HandshakeComplete for unknown handshake");
                return;
            }
        };

        // Finish Noise handshake
        let session = match state.pending.finish(&complete.noise_msg) {
            Ok(s) => s,
            Err(e) => {
                tracing::warn!(error = %e, "pending.finish failed");
                return;
            }
        };
        tracing::debug!(peer_addr = %peer_addr, "noise handshake complete (responder), waiting for chunk_port");

        self.tracker.lock().await.add_responder_waiting_chunk(
            peer_ip,
            session,
            state.chunk_socket,
            state.chunk_socket_port,
            state.peer_pubkey,
        );
    }

    async fn handle_chunk_port_exchange(
        &self,
        data: &[u8],
        peer_addr: SocketAddr,
        peer_ip: Ipv6Addr,
    ) {
        tracing::debug!(peer_addr = %peer_addr, len = data.len(), "received encrypted message (chunk_port exchange)");

        let mut tracker_lock = self.tracker.lock().await;

        if let Some(mut state) = tracker_lock.remove_initiator_waiting(&peer_ip) {
            drop(tracker_lock);

            let mut decrypted = Vec::new();
            if let Err(e) = state.session.decrypt(data, &mut decrypted) {
                tracing::warn!(error = %e, "failed to decrypt peer chunk_port");
                return;
            }
            if decrypted.len() < 2 {
                tracing::warn!("chunk_port message too short");
                return;
            }

            let peer_chunk_port = u16::from_le_bytes([decrypted[0], decrypted[1]]);
            let session_id = state.session.session_id;
            let active_services = default_active_services();

            self.sessions.insert(
                session_id,
                ActiveSession {
                    meta: SessionMeta {
                        session_id,
                        peer_addr,
                        chunk_port: peer_chunk_port,
                        established_at: std::time::Instant::now(),
                        peer_pubkey: state.peer_pubkey,
                        active_services,
                    },
                    crypto: Arc::new(Mutex::new(state.session)),
                    socket: state.chunk_socket,
                    bucket: Arc::new(Mutex::new(TokenBucket::new(Contract::Bulk))),
                },
            );

            tracing::info!(
                peer_addr = %peer_addr,
                session_id = hex::encode(session_id),
                peer_chunk_port,
                "session established (initiator)"
            );
        } else if let Some(mut state) = tracker_lock.remove_responder_waiting(&peer_ip) {
            drop(tracker_lock);

            let mut decrypted = Vec::new();
            if let Err(e) = state.session.decrypt(data, &mut decrypted) {
                tracing::warn!(error = %e, "failed to decrypt initiator chunk_port");
                return;
            }
            if decrypted.len() < 2 {
                tracing::warn!("chunk_port message too short");
                return;
            }

            let peer_chunk_port = u16::from_le_bytes([decrypted[0], decrypted[1]]);

            // Send our chunk_port back
            let chunk_port_msg = state.local_chunk_port.to_le_bytes();
            let mut encrypted = Vec::new();
            if let Err(e) = state.session.encrypt(&chunk_port_msg, &mut encrypted) {
                tracing::warn!(error = %e, "failed to encrypt our chunk_port");
                return;
            }
            if let Err(e) = self.socket.send_to(&encrypted, peer_addr).await {
                tracing::warn!(error = %e, "failed to send our chunk_port");
                return;
            }

            let session_id = state.session.session_id;
            let active_services = default_active_services();

            self.sessions.insert(
                session_id,
                ActiveSession {
                    meta: SessionMeta {
                        session_id,
                        peer_addr,
                        chunk_port: peer_chunk_port,
                        established_at: std::time::Instant::now(),
                        peer_pubkey: state.peer_pubkey,
                        active_services,
                    },
                    crypto: Arc::new(Mutex::new(state.session)),
                    socket: state.chunk_socket,
                    bucket: Arc::new(Mutex::new(TokenBucket::new(Contract::Bulk))),
                },
            );

            tracing::info!(
                peer_addr = %peer_addr,
                session_id = hex::encode(session_id),
                peer_chunk_port,
                "session established (responder)"
            );
        } else {
            drop(tracker_lock);
            tracing::debug!(%peer_addr, len = data.len(), "encrypted message from unknown peer, ignoring");
        }
    }
}
