//! summitd — Summit peer-to-peer daemon.
use std::net::{Ipv6Addr, SocketAddr, SocketAddrV6};
use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context, Result};
use tokio::net::UdpSocket;
use tokio::sync::Mutex;
use zerocopy::AsBytes;

use summit_core::crypto::Keypair;
use summit_core::wire::Contract;

mod cache;
mod capability;
mod chunk;
mod delivery;
mod schema;
mod session;
mod qos;
mod status;

use status::StatusState;
use cache::ChunkCache;
use capability::{broadcast, listener, new_registry};
use session::{new_session_table, ActiveSession, SessionMeta};

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
    .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
    .init();

    let interface = std::env::args().nth(1).unwrap_or_else(|| "veth-a".to_string());
    tracing::info!(interface, "summitd starting");

    let interface_index = broadcast::if_index(&interface)?;

    // Get our link-local address
    let local_link_addr: Ipv6Addr = {
        let probe = std::net::UdpSocket::bind("[::]:0")?;
        let dest = std::net::SocketAddrV6::new(
            "ff02::1".parse()?,
                                               9000,
                                               0,
                                               interface_index,
        );
        probe.connect(dest)?;
        match probe.local_addr()? {
            std::net::SocketAddr::V6(v6) => *v6.ip(),
            _ => anyhow::bail!("expected IPv6 local address"),
        }
    };
    tracing::info!(addr = %local_link_addr, "local link-local address");

    // Bind session socket to our link-local address so responses come back correctly
    let session_listen_socket = UdpSocket::bind(
        SocketAddrV6::new(local_link_addr, 0, 0, interface_index)
    ).await.context("failed to bind session listen socket")?;

    // Generate a fresh keypair for this run
    let keypair = Arc::new(Keypair::generate());
    tracing::info!(public_key = hex::encode(keypair.public), "keypair ready");

    // Build capability announcement with our real public key
    // let test_capability = CapabilityAnnouncement {
    //     capability_hash: summit_core::crypto::hash(b"summit.test"),
    //     public_key:      [0u8; 32],
    //         version:         1,
    //         session_port:    9001,
    //         chunk_port:      9002,
    //         contract:        Contract::Bulk as u8,
    //         flags:           0,
    // };
    //
    // let capabilities = Arc::new(vec![test_capability.clone()]);
    let registry  = new_registry();
    let sessions  = new_session_table();

    // Handshake state tracking (shared between initiator and listener)
    let handshake_tracker = session::HandshakeTracker::shared();

    let session_listen_port = session_listen_socket.local_addr()?.port();

    // let session_send_socket = UdpSocket::bind("[::]:0").await
    // .context("failed to bind session send socket")?;

    let session_listen_socket = Arc::new(session_listen_socket);
    // let session_send_socket = Arc::new(session_send_socket);

    // Create cache (use /tmp for testing, /var/cache/summit for production)
    let cache_root = std::env::var("SUMMIT_CACHE")
    .unwrap_or_else(|_| format!("/tmp/summit-cache-{}", std::process::id()));
    let cache = ChunkCache::new(&cache_root)?;
    tracing::info!(root = %cache_root, "chunk cache initialized");

    // Capability tasks
    let broadcast_task = {
        let keypair = keypair.clone();

        tokio::spawn(async move {
            if let Err(e) = capability::broadcast::broadcast_loop(
                keypair,              // Arc<Keypair>
                interface_index,
                session_listen_port,
            ).await {
                tracing::error!(error = %e, "capability broadcast failed");
            }
        })
    };

    let listener_task = tokio::spawn(listener::listener_loop(
        registry.clone(),
        interface_index,
        keypair.public,
    ));

    let expiry_task = tokio::spawn(listener::expiry_loop(
        registry.clone(),
    ));

    // Get our link-local address by examining the socket's local address
    // Connect to peer's address to determine which local address we use
    let local_link_addr: Ipv6Addr = {
        let probe = std::net::UdpSocket::bind("[::]:0")?;
        // Bind to the multicast group on our interface using scope_id
        let dest = std::net::SocketAddrV6::new(
            "ff02::1".parse()?,
            9000,
            0,
            interface_index,
        );
        probe.connect(dest)?;
        match probe.local_addr()? {
            std::net::SocketAddr::V6(v6) => *v6.ip(),
            _ => anyhow::bail!("expected IPv6 local address"),
        }
    };
    tracing::info!(addr = %local_link_addr, "local link-local address");
    let session_listener = {
        let socket              = session_listen_socket.clone();
        let keypair             = keypair.clone();
        let sessions            = sessions.clone();
        let tracker             = handshake_tracker.clone();
        let local_addr          = local_link_addr;

        tokio::spawn(async move {
            use zerocopy::FromBytes;
            use summit_core::crypto::NoiseResponder;
            use summit_core::wire::{HandshakeInit, HandshakeResponse, HandshakeComplete};

            const HANDSHAKE_INIT_SIZE: usize     = std::mem::size_of::<HandshakeInit>();
            const HANDSHAKE_RESPONSE_SIZE: usize = std::mem::size_of::<HandshakeResponse>();
            const HANDSHAKE_COMPLETE_SIZE: usize = std::mem::size_of::<HandshakeComplete>();

            let mut buf = vec![0u8; 512];
            let mut cleanup_interval = tokio::time::interval(Duration::from_secs(5));

            loop {
                tokio::select! {
                    _ = cleanup_interval.tick() => {
                        tracker.lock().await.cleanup_stale();
                    }

                    result = socket.recv_from(&mut buf) => {
                        let (len, peer_addr) = match result {
                            Ok(r) => r,
                     Err(e) => {
                         tracing::warn!(error = %e, "recv_from failed");
                         continue;
                     }
                        };

                        // Extract IP for tracker lookups
                        let peer_ip = match peer_addr {
                            SocketAddr::V6(v6) => *v6.ip(),
                     _ => { tracing::warn!("ignoring IPv4 peer"); continue; }
                        };

                        // Ignore packets from ourselves (bridge loopback)
                        if peer_ip == local_addr {
                            tracing::trace!("ignoring loopback from own IP");
                            continue;
                        }

                        let data = &buf[..len];

                        if len == HANDSHAKE_INIT_SIZE {
                            let init = match HandshakeInit::read_from(data) {
                                Some(m) => m,
                     None => { tracing::warn!("failed to parse HandshakeInit"); continue; }
                            };

                            tracing::debug!(peer_addr = %peer_addr, "received HandshakeInit");

                            // Deduplicate
                            {
                                let t = tracker.lock().await;
                                if t.has_initiator(&peer_ip) || t.has_initiator_waiting(&peer_ip) {
                                    tracing::debug!(%peer_addr, "already initiating to this peer, ignoring HandshakeInit");
                                    continue;
                                }
                                if t.has_responder(&peer_ip) || t.has_responder_waiting(&peer_ip) {
                                    tracing::debug!(%peer_addr, "duplicate HandshakeInit, ignoring");
                                    continue;
                                }
                            }

                            // Create chunk socket
                            let chunk_socket = match UdpSocket::bind("[::]:0").await {
                                Ok(s) => Arc::new(s),
                     Err(e) => { tracing::warn!(error = %e, "failed to bind chunk socket"); continue; }
                            };
                            let local_chunk_port = match chunk_socket.local_addr() {
                                Ok(addr) => addr.port(),
                     Err(e) => { tracing::warn!(error = %e, "failed to get chunk port"); continue; }
                            };

                            // Create noise responder
                            let noise = match NoiseResponder::new(&keypair) {
                                Ok(n) => n,
                     Err(e) => { tracing::warn!(error = %e, "failed to create noise responder"); continue; }
                            };
                            let responder_nonce = *noise.nonce();

                            // Process Noise message 1
                            let (pending, msg2) = match noise.respond(&init.noise_msg, &init.nonce) {
                                Ok(r) => r,
                     Err(e) => { tracing::warn!(error = %e, "noise.respond failed"); continue; }
                            };

                            // Send HandshakeResponse
                            let response = HandshakeResponse {
                                nonce:     responder_nonce,
                                noise_msg: match msg2.try_into() {
                                    Ok(m) => m,
                     Err(_) => { tracing::warn!("msg2 wrong size"); continue; }
                                },
                            };
                            if let Err(e) = socket.send_to(response.as_bytes(), peer_addr).await {
                                tracing::warn!(error = %e, "failed to send HandshakeResponse");
                                continue;
                            }
                            tracing::debug!(peer_addr = %peer_addr, "sent HandshakeResponse");

                            tracker.lock().await.add_responder(peer_ip, pending, local_chunk_port, chunk_socket);

                        } else if len == HANDSHAKE_RESPONSE_SIZE {
                            let response = match HandshakeResponse::read_from(data) {
                                Some(m) => m,
                     None => { tracing::warn!("failed to parse HandshakeResponse"); continue; }
                            };

                            tracing::debug!(peer_addr = %peer_addr, "received HandshakeResponse");

                            // Deduplicate
                            {
                                let t = tracker.lock().await;
                                if t.has_initiator_waiting(&peer_ip) {
                                    tracing::debug!(%peer_addr, "duplicate HandshakeResponse, ignoring");
                                    continue;
                                }
                            }

                            let state = match tracker.lock().await.remove_initiator(&peer_ip) {
                                Some(s) => s,
                     None => {
                         tracing::warn!(%peer_addr, "HandshakeResponse for unknown handshake");
                         continue;
                     }
                            };

                            // Finish Noise handshake
                            let (mut session, msg3) = match state.noise.finish(&response.noise_msg, &response.nonce) {
                                Ok(r) => r,
                     Err(e) => { tracing::warn!(error = %e, "noise.finish failed"); continue; }
                            };

                            // Send HandshakeComplete
                            let complete = HandshakeComplete {
                                noise_msg: match msg3.try_into() {
                                    Ok(m) => m,
                     Err(_) => { tracing::warn!("msg3 wrong size"); continue; }
                                },
                            };
                            if let Err(e) = socket.send_to(complete.as_bytes(), peer_addr).await {
                                tracing::warn!(error = %e, "failed to send HandshakeComplete");
                                continue;
                            }
                            tracing::debug!(peer_addr = %peer_addr, "sent HandshakeComplete");

                            // Send our chunk_port encrypted
                            let chunk_port_msg = state.chunk_socket_port.to_le_bytes();
                            let mut encrypted = Vec::new();
                            if let Err(e) = session.encrypt(&chunk_port_msg, &mut encrypted) {
                                tracing::warn!(error = %e, "failed to encrypt chunk_port"); continue;
                            }
                            if let Err(e) = socket.send_to(&encrypted, peer_addr).await {
                                tracing::warn!(error = %e, "failed to send chunk_port"); continue;
                            }
                            tracing::debug!(peer_addr = %peer_addr, "sent chunk_port (initiator)");

                            tracker.lock().await.add_initiator_waiting_chunk(
                                peer_ip, session, state.chunk_socket, state.chunk_socket_port
                            );

                        } else if len == HANDSHAKE_COMPLETE_SIZE {
                            let complete = match HandshakeComplete::read_from(data) {
                                Some(m) => m,
                     None => { tracing::warn!("failed to parse HandshakeComplete"); continue; }
                            };

                            tracing::debug!(peer_addr = %peer_addr, "received HandshakeComplete");

                            let state = match tracker.lock().await.remove_responder(&peer_ip) {
                                Some(s) => s,
                     None => {
                         tracing::warn!(%peer_addr, "HandshakeComplete for unknown handshake");
                         continue;
                     }
                            };

                            // Finish Noise handshake
                            let session = match state.pending.finish(&complete.noise_msg) {
                                Ok(s) => s,
                     Err(e) => { tracing::warn!(error = %e, "pending.finish failed"); continue; }
                            };
                            tracing::debug!(peer_addr = %peer_addr, "noise handshake complete (responder), waiting for chunk_port");

                            tracker.lock().await.add_responder_waiting_chunk(
                                peer_ip, session, state.chunk_socket, state.chunk_socket_port
                            );

                        } else {
                            // Encrypted message — chunk_port exchange
                            tracing::debug!(peer_addr = %peer_addr, len, "received encrypted message (chunk_port exchange)");

                            let mut tracker_lock = tracker.lock().await;

                            if let Some(mut state) = tracker_lock.remove_initiator_waiting(&peer_ip) {
                                drop(tracker_lock);

                                let mut decrypted = Vec::new();
                                if let Err(e) = state.session.decrypt(data, &mut decrypted) {
                                    tracing::warn!(error = %e, "failed to decrypt peer chunk_port"); continue;
                                }
                                if decrypted.len() < 2 {
                                    tracing::warn!("chunk_port message too short"); continue;
                                }

                                let peer_chunk_port = u16::from_le_bytes([decrypted[0], decrypted[1]]);
                                let session_id = state.session.session_id;

                                sessions.insert(session_id, ActiveSession {
                                    meta: SessionMeta {
                                        session_id,
                                        peer_addr,
                                        chunk_port: peer_chunk_port,
                                        contract: Contract::Bulk,
                                        established_at: std::time::Instant::now(),
                                    },
                                    crypto: Arc::new(Mutex::new(state.session)),
                                    socket: state.chunk_socket,
                                    bucket: Mutex::new(qos::TokenBucket::new(Contract::Bulk)),
                                });

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
                                    tracing::warn!(error = %e, "failed to decrypt initiator chunk_port"); continue;
                                }
                                if decrypted.len() < 2 {
                                    tracing::warn!("chunk_port message too short"); continue;
                                }

                                let peer_chunk_port = u16::from_le_bytes([decrypted[0], decrypted[1]]);

                                // Send our chunk_port back
                                let chunk_port_msg = state.local_chunk_port.to_le_bytes();
                                let mut encrypted = Vec::new();
                                if let Err(e) = state.session.encrypt(&chunk_port_msg, &mut encrypted) {
                                    tracing::warn!(error = %e, "failed to encrypt our chunk_port"); continue;
                                }
                                if let Err(e) = socket.send_to(&encrypted, peer_addr).await {
                                    tracing::warn!(error = %e, "failed to send our chunk_port"); continue;
                                }

                                let session_id = state.session.session_id;

                                sessions.insert(session_id, ActiveSession {
                                    meta: SessionMeta {
                                        session_id,
                                        peer_addr,
                                        chunk_port: peer_chunk_port,
                                        contract: Contract::Bulk,
                                        established_at: std::time::Instant::now(),
                                    },
                                    crypto: Arc::new(Mutex::new(state.session)),
                                    socket: state.chunk_socket,
                                    bucket: Mutex::new(qos::TokenBucket::new(Contract::Bulk)),
                                });

                                tracing::info!(
                                    peer_addr = %peer_addr,
                                    session_id = hex::encode(session_id),
                                               peer_chunk_port,
                                               "session established (responder)"
                                );

                            } else {
                                drop(tracker_lock);
                                tracing::debug!(%peer_addr, len, "encrypted message from unknown peer, ignoring");
                            }
                        }
                    }
                }
            }
        })
    };

    let session_initiator = {
        let socket = session_listen_socket.clone();
        let keypair  = keypair.clone();
        let registry = registry.clone();
        let tracker  = handshake_tracker.clone();

        tokio::spawn(async move {
            use summit_core::crypto::NoiseInitiator;
            use summit_core::wire::HandshakeInit;

            let mut interval = tokio::time::interval(Duration::from_secs(3));
            let mut attempted = std::collections::HashSet::new();

            loop {
                interval.tick().await;
                tracing::debug!(peers = registry.len(), "initiator tick");

                for peer in registry.iter() {
                    let peer_pubkey: [u8; 32] = *peer.key();
                    let entry = peer.value();

                    if attempted.contains(&peer_pubkey) {
                        continue;
                    }

                    // Only initiate if our public key is lower than peer's
                    if keypair.public >= entry.public_key {
                        tracing::debug!(
                            our_key = hex::encode(&keypair.public[..4]),
                                        peer_key = hex::encode(&entry.public_key[..4]),
                                        "peer has lower key, waiting"
                        );
                        continue;
                    }
                    tracing::debug!(
                        our_key = hex::encode(&keypair.public[..4]),
                                    peer_key = hex::encode(&entry.public_key[..4]),
                                    "we have lower key, initiating"
                    );

                    // Only mark attempted if we're actually initiating
                    attempted.insert(peer_pubkey);

                    let peer_addr = SocketAddr::V6(SocketAddrV6::new(
                        entry.addr,
                        entry.session_port,
                        0,
                        interface_index,
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
                    let (noise, msg1) = match NoiseInitiator::new(&keypair) {
                        Ok(r) => r,
                     Err(e) => {
                         tracing::warn!(error = %e, "failed to create noise initiator");
                         continue;
                     }
                    };

                    // Build HandshakeInit
                    let init = HandshakeInit {
                        capability_hash: summit_core::crypto::hash(b"summit.test"),
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
                    if let Err(e) = socket.send_to(init.as_bytes(), peer_addr).await {
                        tracing::warn!(error = %e, "failed to send HandshakeInit");
                        continue;
                    }


                    let peer_ip = entry.addr;
                    // Track this initiated handshake
                    tracker.lock().await.add_initiator(peer_ip, noise, chunk_socket, local_chunk_port);
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
    // Delivery tracker
    let delivery_tracker = delivery::DeliveryTracker::new();

    // Spawn chunk send/receive tasks for each active session
    let chunk_manager = {
        let sessions = sessions.clone();
        let cache = cache.clone();
        let tracker = delivery_tracker.clone();

        tokio::spawn(async move {
            let mut interval = tokio::time::interval(Duration::from_secs(1));
            let mut seen_sessions = std::collections::HashSet::new();

            loop {
                interval.tick().await;
                tracing::debug!(session_count = sessions.len(), "chunk manager tick");

                for entry in sessions.iter() {
                    let session_id = *entry.key();
                    let active = entry.value();

                    if seen_sessions.contains(&session_id) {
                        continue;
                    }
                    seen_sessions.insert(session_id);

                    tracing::info!(session_id = hex::encode(session_id), "spawning chunk tasks for session");

                    let peer_addr = active.meta.peer_addr;
                    let crypto = active.crypto.clone();
                    let socket = active.socket.clone();

                    // Create channel for received chunks
                    let (chunk_tx, mut chunk_rx) = tokio::sync::mpsc::channel::<chunk::IncomingChunk>(100);

                    // Spawn receiver printer (logs received chunks)
                    tokio::spawn(async move {
                        while let Some(chunk) = chunk_rx.recv().await {
                            tracing::info!(
                                content_hash = hex::encode(chunk.content_hash),
                                           type_tag = chunk.type_tag,
                                           payload = %String::from_utf8_lossy(&chunk.payload),
                                           "RECEIVED CHUNK"
                            );
                        }
                    });

                    // Spawn receive loop
                    let recv_socket = socket.clone();
                    let recv_crypto = crypto.clone();
                    let recv_cache = cache.clone();
                    let recv_tracker = tracker.clone();
                    let peer_addr_str = peer_addr.to_string();

                    tokio::spawn(async move {
                        if let Err(e) = chunk::receive::receive_loop(
                            recv_socket,
                            recv_crypto,
                            chunk_tx,
                            recv_cache,
                            recv_tracker,
                            peer_addr_str,
                        ).await {
                            tracing::warn!(error = %e, "receive loop terminated");
                        }
                    });
                }
            }
        })
    };

    // Multipath broadcast sender - sends each chunk to ALL sessions
    let broadcast_sender = {
        let sessions = sessions.clone();
        let cache = cache.clone();

        tokio::spawn(async move {
            let mut counter = 0u64;
            let mut interval = tokio::time::interval(Duration::from_secs(2));

            loop {
                interval.tick().await;
                counter += 1;

                let session_count = sessions.len();
                if session_count == 0 {
                    tracing::debug!("no sessions, skipping broadcast");
                    continue;
                }

                let payload = format!("ping #{}", counter);
                let chunk = chunk::OutgoingChunk {
                    type_tag:  1,
                    schema_id: schema::KnownSchema::TestPing.id(),
                     payload:   bytes::Bytes::from(payload),
                };

                // Priority check — suppress Background if any Realtime session exists
                let has_realtime = sessions.iter()
                .any(|e| matches!(e.value().meta.contract, Contract::Realtime));


                let mut send_tasks = Vec::new();

                for entry in sessions.iter() {
                    let peer_addr = entry.value().meta.peer_addr;
                    let chunk_port = entry.value().meta.chunk_port;  // Get from session metadata
                    let socket = entry.value().socket.clone();
                    let crypto = entry.value().crypto.clone();
                    let contract    = entry.value().meta.contract;

                    // Drop Background if Realtime is active
                    if has_realtime && matches!(contract, Contract::Background) {
                        tracing::debug!(%peer_addr, "background chunk suppressed — realtime active");
                        continue;
                    }

                    // Check token bucket — drop if rate limited
                    let allowed = entry.value().bucket.lock().await.allow();
                    if !allowed {
                        tracing::debug!(
                            %peer_addr,
                            ?contract,
                            "chunk dropped — rate limited"
                        );
                        continue;
                    }

                    // Construct chunk peer address using chunk_port from session
                    let chunk_peer_addr = match peer_addr {
                        std::net::SocketAddr::V6(mut addr) => {
                            addr.set_port(chunk_port);  // use session's chunk_port
                            std::net::SocketAddr::V6(addr)
                        }
                        _ => peer_addr,
                    };

                    let chunk_clone = chunk.clone();
                    let cache_clone = cache.clone();

                    let task = tokio::spawn(async move {
                        chunk::send::send_chunk(
                            socket,
                            chunk_peer_addr,
                            crypto,
                            chunk_clone,
                            cache_clone,
                        ).await
                    });

                    send_tasks.push(task);
                }

                // ... rest of broadcast sender
            }
        })
    };

    // Delivery stats printer - shows multipath deliveries every 10 seconds
    let stats_printer = {
        let tracker = delivery_tracker.clone();
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(Duration::from_secs(10));
            loop {
                interval.tick().await;
                tracker.print_stats();
            }
        })
    };

    // Status HTTP endpoint
    let status_port = 9001u16;
    let status_server = {
        let state = StatusState {
            sessions: sessions.clone(),
            cache:    cache.clone(),
            registry: registry.clone(),
        };
        tokio::spawn(async move {
            if let Err(e) = status::serve(state, status_port).await {
                tracing::error!(error = %e, "status server failed");
            }
        })
    };

    tokio::select! {
        r = broadcast_task      => tracing::error!("broadcast task exited: {:?}", r),
        r = listener_task       => tracing::error!("listener task exited: {:?}", r),
        r = expiry_task         => tracing::error!("expiry task exited: {:?}", r),
        r = session_listener    => tracing::error!("session listener exited: {:?}", r),
        r = session_initiator   => tracing::error!("session initiator exited: {:?}", r),
        r = session_printer     => tracing::error!("session printer exited: {:?}", r),
        r = chunk_manager       => tracing::error!("chunk manager exited: {:?}", r),
        r = broadcast_sender    => tracing::error!("broadcast sender exited: {:?}", r),
        r = stats_printer       => tracing::error!("stats printer exited: {:?}", r),
    }

    Ok(())
}
