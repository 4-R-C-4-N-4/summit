//! Proactive session initiator.
//!
//! Periodically scans the peer registry and initiates Noise_XX
//! handshakes with discovered peers (on a 3-second interval).
//!
//! Normally only the peer with the lower public key initiates. If that hasn't
//! produced a session within [`FALLBACK_INITIATE_AFTER`], the higher-key peer
//! initiates as well: with one-way multicast the lower-key side may never hear
//! us, so it can never initiate. Simultaneous attempts are resolved in the
//! listener (see `should_yield_to_incoming`).

use std::collections::{HashMap, HashSet};
use std::net::{Ipv6Addr, SocketAddr, SocketAddrV6};
use std::sync::Arc;
use std::time::{Duration, Instant};

use tokio::net::UdpSocket;
use tokio::sync::broadcast;
use zerocopy::AsBytes;

use summit_core::crypto::{Keypair, NoiseInitiator};
use summit_core::wire::HandshakeInit;
use summit_services::{PeerRegistry, SessionTable};

use super::state::SharedTracker;
use super::{is_designated_initiator, FALLBACK_INITIATE_AFTER};

pub struct SessionInitiator {
    socket: Arc<UdpSocket>,
    keypair: Arc<Keypair>,
    registry: PeerRegistry,
    tracker: SharedTracker,
    sessions: SessionTable,
    interface_index: u32,
    shutdown: broadcast::Receiver<()>,
    /// When each known peer was first seen without a session.
    waiting_since: HashMap<[u8; 32], Instant>,
}

impl SessionInitiator {
    pub fn new(
        socket: Arc<UdpSocket>,
        keypair: Arc<Keypair>,
        registry: PeerRegistry,
        tracker: SharedTracker,
        sessions: SessionTable,
        interface_index: u32,
        shutdown: broadcast::Receiver<()>,
    ) -> Self {
        Self {
            socket,
            keypair,
            registry,
            tracker,
            sessions,
            interface_index,
            shutdown,
            waiting_since: HashMap::new(),
        }
    }

    pub async fn run(mut self) -> anyhow::Result<()> {
        let mut interval = tokio::time::interval(Duration::from_secs(3));

        loop {
            tokio::select! {
                _ = self.shutdown.recv() => {
                    tracing::info!("session initiator shutting down");
                    return Ok(());
                }

                _ = interval.tick() => {
                    tracing::debug!(peers = self.registry.len(), "initiator tick");
                    self.initiate_handshakes().await;
                }
            }
        }
    }

    async fn initiate_handshakes(&mut self) {
        let active_pubkeys: HashSet<[u8; 32]> = self
            .sessions
            .iter()
            .map(|entry| entry.value().meta.peer_pubkey)
            .collect();

        // Snapshot the registry so no DashMap guard is held across an await
        // (the listeners write to it concurrently).
        let peers: Vec<([u8; 32], Ipv6Addr, u16)> = self
            .registry
            .iter()
            .map(|e| (*e.key(), e.value().addr, e.value().session_port))
            .collect();

        let now = Instant::now();
        let known: HashSet<[u8; 32]> = peers.iter().map(|p| p.0).collect();
        self.waiting_since
            .retain(|k, _| known.contains(k) && !active_pubkeys.contains(k));

        for (peer_pubkey, peer_ip, session_port) in peers {
            if active_pubkeys.contains(&peer_pubkey) {
                continue;
            }
            let waited = now - *self.waiting_since.entry(peer_pubkey).or_insert(now);

            if self.tracker.lock().await.in_progress(&peer_ip) {
                continue;
            }

            if !is_designated_initiator(&self.keypair.public, &peer_pubkey) {
                if waited < FALLBACK_INITIATE_AFTER {
                    tracing::debug!(
                        peer = hex::encode(&peer_pubkey[..4]),
                        "peer has lower key, waiting for it to initiate"
                    );
                    continue;
                }
                tracing::info!(
                    peer = hex::encode(&peer_pubkey[..8]),
                    waited_secs = waited.as_secs(),
                    "peer has not initiated; initiating as fallback (it may not hear our announcements)"
                );
            }

            self.initiate(peer_pubkey, peer_ip, session_port).await;
        }
    }

    async fn initiate(&self, peer_pubkey: [u8; 32], peer_ip: Ipv6Addr, session_port: u16) {
        let peer_addr = SocketAddr::V6(SocketAddrV6::new(
            peer_ip,
            session_port,
            0,
            self.interface_index,
        ));

        tracing::debug!(peer_addr = %peer_addr, "initiating handshake");

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
                tracing::warn!(error = %e, "failed to get chunk socket addr");
                return;
            }
        };

        // Create noise initiator
        let (noise, msg1) = match NoiseInitiator::new(&self.keypair) {
            Ok(r) => r,
            Err(e) => {
                tracing::warn!(error = %e, "failed to create noise initiator");
                return;
            }
        };

        // Build HandshakeInit
        let init = HandshakeInit {
            service_hash: summit_core::wire::service_hash(b"summit.file_transfer"),
            noise_msg: match msg1.try_into() {
                Ok(m) => m,
                Err(_) => {
                    tracing::warn!("msg1 wrong size");
                    return;
                }
            },
            nonce: *noise.nonce(),
        };

        // Register before sending: the response (or a simultaneous Init from
        // the peer) can arrive before send_to returns.
        self.tracker.lock().await.add_initiator(
            peer_ip,
            peer_pubkey,
            noise,
            chunk_socket,
            local_chunk_port,
        );

        if let Err(e) = self.socket.send_to(init.as_bytes(), peer_addr).await {
            tracing::warn!(error = %e, "failed to send HandshakeInit");
            self.tracker.lock().await.remove_initiator(&peer_ip);
        }
    }
}
