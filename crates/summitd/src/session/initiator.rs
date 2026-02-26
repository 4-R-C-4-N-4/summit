//! Proactive session initiator.
//!
//! Periodically scans the peer registry and initiates Noise_XX
//! handshakes with discovered peers (on a 3-second interval).

use std::net::{SocketAddr, SocketAddrV6};
use std::sync::Arc;
use std::time::Duration;

use tokio::net::UdpSocket;
use tokio::sync::broadcast;
use zerocopy::AsBytes;

use summit_core::crypto::{Keypair, NoiseInitiator};
use summit_core::wire::HandshakeInit;
use summit_services::{PeerRegistry, SessionTable};

use super::state::SharedTracker;

pub struct SessionInitiator {
    socket: Arc<UdpSocket>,
    keypair: Arc<Keypair>,
    registry: PeerRegistry,
    tracker: SharedTracker,
    sessions: SessionTable,
    interface_index: u32,
    shutdown: broadcast::Receiver<()>,
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

    async fn initiate_handshakes(&self) {
        // Collect pubkeys that already have an active session
        let active_pubkeys: std::collections::HashSet<[u8; 32]> = self
            .sessions
            .iter()
            .map(|entry| entry.value().meta.peer_pubkey)
            .collect();

        for peer in self.registry.iter() {
            let peer_pubkey: [u8; 32] = *peer.key();
            let entry = peer.value();

            // Skip if we already have an active session with this peer
            if active_pubkeys.contains(&peer_pubkey) {
                continue;
            }

            // Skip if a handshake is already in progress for this peer's IP
            {
                let tracker = self.tracker.lock().await;
                if tracker.has_initiator(&entry.addr) || tracker.has_initiator_waiting(&entry.addr)
                {
                    continue;
                }
            }

            // Only initiate if our public key is lower than peer's
            if self.keypair.public >= entry.public_key {
                tracing::debug!(
                    our_key = hex::encode(&self.keypair.public[..4]),
                    peer_key = hex::encode(&entry.public_key[..4]),
                    "peer has lower key, waiting"
                );
                continue;
            }
            tracing::debug!(
                our_key = hex::encode(&self.keypair.public[..4]),
                peer_key = hex::encode(&entry.public_key[..4]),
                "we have lower key, initiating"
            );

            let peer_addr = SocketAddr::V6(SocketAddrV6::new(
                entry.addr,
                entry.session_port,
                0,
                self.interface_index,
            ));

            tracing::debug!(peer_addr = %peer_addr, "initiating handshake");

            // Create chunk socket
            let chunk_socket = match UdpSocket::bind("[::]:0").await {
                Ok(s) => Arc::new(s),
                Err(e) => {
                    tracing::warn!(error = %e, "failed to bind chunk socket");
                    continue;
                }
            };

            let local_chunk_port = match chunk_socket.local_addr() {
                Ok(addr) => addr.port(),
                Err(e) => {
                    tracing::warn!(error = %e, "failed to get chunk socket addr");
                    continue;
                }
            };

            // Create noise initiator
            let (noise, msg1) = match NoiseInitiator::new(&self.keypair) {
                Ok(r) => r,
                Err(e) => {
                    tracing::warn!(error = %e, "failed to create noise initiator");
                    continue;
                }
            };

            // Build HandshakeInit
            let init = HandshakeInit {
                service_hash: summit_core::wire::service_hash(b"summit.file_transfer"),
                noise_msg: match msg1.try_into() {
                    Ok(m) => m,
                    Err(_) => {
                        tracing::warn!("msg1 wrong size");
                        continue;
                    }
                },
                nonce: *noise.nonce(),
            };

            // Send HandshakeInit
            if let Err(e) = self.socket.send_to(init.as_bytes(), peer_addr).await {
                tracing::warn!(error = %e, "failed to send HandshakeInit");
                continue;
            }

            let peer_ip = entry.addr;
            self.tracker.lock().await.add_initiator(
                peer_ip,
                peer_pubkey,
                noise,
                chunk_socket,
                local_chunk_port,
            );
        }
    }
}
