//! summitd — Summit peer-to-peer daemon.

mod capability;
mod session;

use std::net::{Ipv6Addr, SocketAddrV6};
use std::sync::Arc;
use std::time::Duration;

use anyhow::Result;

use summit_core::crypto::Keypair;
use summit_core::wire::{CapabilityAnnouncement, Contract, HandshakeInit};

use capability::{broadcast, listener, new_registry};
use session::{handshake, new_session_table};

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
    .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
    .init();

    let interface = std::env::args().nth(1).unwrap_or_else(|| "veth-a".to_string());
    tracing::info!(interface, "summitd starting");

    let interface_index = broadcast::if_index(&interface)?;

    // Generate a fresh keypair for this run
    let keypair = Arc::new(Keypair::generate());
    tracing::info!(public_key = hex::encode(keypair.public), "keypair ready");

    // Build capability announcement with our real public key
    let test_capability = CapabilityAnnouncement {
        capability_hash: summit_core::crypto::hash(b"summit.test.ping"),
        public_key:      keypair.public,
            version:         1,
            session_port:    9001,
            contract:        Contract::Bulk as u8,
            flags:           0,
    };

    let capabilities = Arc::new(vec![test_capability.clone()]);
    let registry  = new_registry();
    let sessions  = new_session_table();

    // Two sockets — listener owns port 9001, sender uses ephemeral port
    let session_listen_socket = {
        let bind_addr = SocketAddrV6::new(Ipv6Addr::UNSPECIFIED, 9001, 0, interface_index);
        Arc::new(
            tokio::net::UdpSocket::bind(bind_addr)
            .await
            .map_err(|e| anyhow::anyhow!("failed to bind session listen socket: {e}"))?
        )
    };

    let session_send_socket = {
        let bind_addr = SocketAddrV6::new(Ipv6Addr::UNSPECIFIED, 0, 0, interface_index);
        Arc::new(
            tokio::net::UdpSocket::bind(bind_addr)
            .await
            .map_err(|e| anyhow::anyhow!("failed to bind session send socket: {e}"))?
        )
    };

    tracing::info!("session sockets bound");

    // Capability tasks
    let broadcast_task = tokio::spawn(broadcast::broadcast_loop(
        capabilities.clone(),
                                                                interface_index,
    ));
    let listener_task = tokio::spawn(listener::listener_loop(
        registry.clone(),
                                                             interface_index,
    ));
    let expiry_task = tokio::spawn(listener::expiry_loop(
        registry.clone(),
    ));

    // Session listener — accepts incoming handshakes on port 9001
    let session_listener = {
        let socket   = session_listen_socket.clone();
        let keypair  = keypair.clone();
        let sessions = sessions.clone();
        tokio::spawn(async move {
            let mut buf = vec![0u8; 512];
            loop {
                match socket.recv_from(&mut buf).await {
                    Ok((len, peer_addr)) => {
                        // Only handle packets that could be HandshakeInit (80 bytes)
                        if len != std::mem::size_of::<HandshakeInit>() {
                            tracing::trace!(len, %peer_addr, "ignoring non-HandshakeInit packet");
                            continue;
                        }
                        tracing::debug!(%peer_addr, "incoming handshake");
                        let keypair  = keypair.clone();
                        let sessions = sessions.clone();
                        let data     = buf[..len].to_vec();
                        tokio::spawn(async move {
                            if let Err(e) = handshake::respond(
                                peer_addr,
                                &data,
                                &keypair,
                                sessions,
                                Contract::Bulk,
                            ).await {
                                tracing::warn!(error = %e, "handshake respond failed");
                            }
                        });
                    }
                    Err(e) => tracing::warn!(error = %e, "session socket recv failed"),
                }
            }
        })
    };

    // Session initiator — watches registry, connects to new peers
    let session_initiator = {
        let socket   = session_send_socket.clone();
        let keypair  = keypair.clone();
        let registry = registry.clone();
        let sessions = sessions.clone();
        let our_key  = keypair.public;
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(Duration::from_secs(3));
            loop {
                interval.tick().await;
                tracing::debug!(peers = registry.len(), "initiator tick");

                // In the initiator loop in main.rs, replace the inner for loop
                for peer in registry.iter() {
                    let cap_hash = *peer.key();
                    let entry = peer.value().clone();

                    if entry.public_key == our_key {
                        continue;
                    }
                    if entry.public_key >= our_key {
                        continue;
                    }

                    let already_connected = sessions.iter().any(|s| {
                        match (s.value().meta.peer_addr, std::net::SocketAddr::V6(SocketAddrV6::new(entry.addr, 0, 0, 0))) {
                            (std::net::SocketAddr::V6(a), std::net::SocketAddr::V6(b)) => a.ip() == b.ip(),
                                                                _ => false,
                        }
                    });
                    if already_connected {
                        continue;
                    }

                    // Found a peer to connect to — do it and break
                    let peer_addr = std::net::SocketAddr::V6(
                        SocketAddrV6::new(entry.addr, entry.session_port, 0, interface_index)
                    );

                    let announcement = CapabilityAnnouncement {
                        capability_hash: cap_hash,
                        public_key:      entry.public_key,
                            version:         entry.version,
                            session_port:    entry.session_port,
                            contract:        entry.contract as u8,
                            flags:           0,
                    };

                    let socket   = socket.clone();
                    let keypair  = keypair.clone();
                    let sessions = sessions.clone();

                    tokio::spawn(async move {
                        if let Err(e) = handshake::initiate(
                            socket,
                            peer_addr,
                            &announcement,
                            &keypair,
                            sessions,
                            Contract::Bulk,
                        ).await {
                            tracing::warn!(error = %e, "handshake initiate failed");
                        }
                    });

                    break;  // Only one connection attempt per tick
                }
            }
        })
    };

    // Print session table every 5 seconds
    let session_printer = {
        let sessions = sessions.clone();
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(Duration::from_secs(5));
            loop {
                interval.tick().await;
                tracing::info!(count = sessions.len(), "session table snapshot");
                for s in sessions.iter() {
                    tracing::info!(
                        session_id = hex::encode(s.meta.session_id),
                                   peer = %s.meta.peer_addr,
                                   "  session"
                    );
                }
            }
        })
    };

    tokio::select! {
        r = broadcast_task    => tracing::error!("broadcast task exited: {:?}", r),
        r = listener_task     => tracing::error!("listener task exited: {:?}", r),
        r = expiry_task       => tracing::error!("expiry task exited: {:?}", r),
        r = session_listener  => tracing::error!("session listener exited: {:?}", r),
        r = session_initiator => tracing::error!("session initiator exited: {:?}", r),
        r = session_printer   => tracing::error!("session printer exited: {:?}", r),
    }

    Ok(())
}
