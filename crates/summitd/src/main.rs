//! summitd — Summit peer-to-peer daemon.

use std::net::{Ipv6Addr, SocketAddrV6};
use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context, Result};
use tokio::net::UdpSocket;

use summit_core::config::{data_dir, SummitConfig};
use summit_core::crypto::Keypair;
use summit_core::wire::{service_hash, Contract};

use summit_services::{
    new_registry, new_session_table, ChunkCache, ComputeStore, FileReassembler, MessageStore,
    SendTarget, TrustRegistry, UntrustedBuffer,
};

mod capability;
mod chunk;
mod delivery;
mod dispatch;
mod session;

use capability::{broadcast, listener};
use tokio::sync::mpsc;

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();

    // Load config
    if let Err(e) = SummitConfig::write_default_if_missing() {
        tracing::warn!(error = %e, "failed to write default config");
    }
    let config = SummitConfig::load().unwrap_or_else(|e| {
        tracing::warn!(error = %e, "failed to load config, using defaults");
        SummitConfig::default()
    });

    let interface = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "veth-a".to_string());
    tracing::info!(interface, "summitd starting");

    let interface_index = broadcast::if_index(&interface)?;

    // Get our link-local address
    let local_link_addr: Ipv6Addr = {
        let probe = std::net::UdpSocket::bind("[::]:0")?;
        let dest = std::net::SocketAddrV6::new("ff02::1".parse()?, 9000, 0, interface_index);
        probe.connect(dest)?;
        match probe.local_addr()? {
            std::net::SocketAddr::V6(v6) => *v6.ip(),
            _ => anyhow::bail!("expected IPv6 local address"),
        }
    };
    tracing::info!(addr = %local_link_addr, "local link-local address");

    // Bind session socket
    let session_listen_socket = Arc::new(
        UdpSocket::bind(SocketAddrV6::new(local_link_addr, 0, 0, interface_index))
            .await
            .context("failed to bind session listen socket")?,
    );
    let session_listen_port = session_listen_socket.local_addr()?.port();

    // Keypair
    let keypair = Arc::new(Keypair::generate());
    tracing::info!(public_key = hex::encode(keypair.public), "keypair ready");

    // Shared state
    let registry = new_registry();
    let sessions = new_session_table();
    let handshake_tracker = session::HandshakeTracker::shared();
    let message_store = MessageStore::new();
    let compute_store = ComputeStore::new();

    // Chunk cache
    let cache_root = std::env::var("SUMMIT_CACHE")
        .unwrap_or_else(|_| data_dir().join("cache").to_string_lossy().into_owned());
    let cache = ChunkCache::new(&cache_root)?;
    tracing::info!(root = %cache_root, "chunk cache initialized");

    // Trust
    let trust_registry = TrustRegistry::new();
    trust_registry.apply_config(config.trust.auto_trust, &config.trust.trusted_peers);
    if config.trust.auto_trust {
        tracing::warn!("auto-trust enabled — all discovered peers will be trusted");
    }
    let untrusted_buffer = UntrustedBuffer::new();

    // Outbound chunk queue
    let (chunk_tx, chunk_rx) = mpsc::unbounded_channel::<(SendTarget, chunk::OutgoingChunk)>();

    // File reassembler
    let file_transfer_path = config.services.file_transfer_settings.storage_path.clone();
    tracing::info!(path = %file_transfer_path.display(), "file transfer storage path");
    let reassembler = Arc::new(FileReassembler::new(file_transfer_path.clone()));

    // Service dispatcher
    let dispatcher = {
        use dispatch::ServiceDispatcher;
        use summit_services::{ChunkService, ComputeService, KnownSchema, MessagingService};
        let mut d = ServiceDispatcher::new();
        let reassembler_svc = reassembler.clone() as Arc<dyn ChunkService>;
        d.register(reassembler_svc.clone());
        d.register_schema(KnownSchema::FileData.id(), reassembler_svc.clone());
        d.register_schema(KnownSchema::FileMetadata.id(), reassembler_svc);
        let messaging = Arc::new(MessagingService::new(message_store.clone()));
        d.register(messaging as Arc<dyn ChunkService>);
        if config.services.compute {
            let compute_svc = Arc::new(ComputeService::new(
                compute_store.clone(),
                config.services.compute_settings.clone(),
                chunk_tx.clone(),
            ));
            d.register(compute_svc as Arc<dyn ChunkService>);
        }
        Arc::new(d)
    };

    // Broadcast services list
    let mut broadcast_services = Vec::new();
    if config.services.file_transfer {
        broadcast_services.push(broadcast::ServiceEntry {
            hash: service_hash(b"summit.file_transfer"),
            contract: Contract::Bulk,
            chunk_port: 0,
        });
    }
    if config.services.messaging {
        broadcast_services.push(broadcast::ServiceEntry {
            hash: service_hash(b"summit.messaging"),
            contract: Contract::Bulk,
            chunk_port: 0,
        });
    }
    if config.services.stream_udp {
        broadcast_services.push(broadcast::ServiceEntry {
            hash: service_hash(b"summit.stream_udp"),
            contract: Contract::Realtime,
            chunk_port: config.network.chunk_port,
        });
    }
    if config.services.compute {
        broadcast_services.push(broadcast::ServiceEntry {
            hash: service_hash(b"summit.compute"),
            contract: Contract::Bulk,
            chunk_port: 0,
        });
    }
    tracing::info!(
        file_transfer = config.services.file_transfer,
        messaging = config.services.messaging,
        stream_udp = config.services.stream_udp,
        compute = config.services.compute,
        "services enabled"
    );

    // ── Shutdown channel ─────────────────────────────────────────────────────
    let (shutdown_tx, _) = tokio::sync::broadcast::channel::<()>(1);

    {
        let shutdown = shutdown_tx.clone();
        tokio::spawn(async move {
            tokio::signal::ctrl_c().await.ok();
            tracing::info!("shutdown signal received");
            let _ = shutdown.send(());
        });
    }

    // ── Spawn tasks ──────────────────────────────────────────────────────────

    let broadcast_task = {
        let keypair = keypair.clone();
        tokio::spawn(async move {
            if let Err(e) = broadcast::broadcast_loop(
                keypair,
                interface_index,
                session_listen_port,
                broadcast_services,
            )
            .await
            {
                tracing::error!(error = %e, "capability broadcast failed");
            }
        })
    };

    let listener_task = tokio::spawn(listener::listener_loop(
        registry.clone(),
        interface_index,
        keypair.public,
    ));

    let expiry_task = tokio::spawn(listener::expiry_loop(registry.clone()));

    let session_listener_task = tokio::spawn(
        session::listener::SessionListener::new(
            session_listen_socket.clone(),
            keypair.clone(),
            sessions.clone(),
            handshake_tracker.clone(),
            local_link_addr,
            registry.clone(),
            shutdown_tx.subscribe(),
        )
        .run(),
    );

    let session_initiator_task = tokio::spawn(
        session::initiator::SessionInitiator::new(
            session_listen_socket,
            keypair.clone(),
            registry.clone(),
            handshake_tracker,
            sessions.clone(),
            interface_index,
            shutdown_tx.subscribe(),
        )
        .run(),
    );

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

    let delivery_tracker = delivery::DeliveryTracker::new();

    let chunk_manager_task = tokio::spawn(
        chunk::manager::ChunkManager::new(
            sessions.clone(),
            cache.clone(),
            delivery_tracker.clone(),
            reassembler.clone(),
            trust_registry.clone(),
            untrusted_buffer.clone(),
            dispatcher.clone(),
            shutdown_tx.subscribe(),
        )
        .run(),
    );

    let send_worker_task = tokio::spawn(
        chunk::send_worker::SendWorker::new(
            sessions.clone(),
            cache.clone(),
            trust_registry.clone(),
            chunk_rx,
            shutdown_tx.subscribe(),
        )
        .run(),
    );

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
    let status_port = config.network.api_port;
    let _status_server = {
        let mut enabled_services: Vec<String> = Vec::new();
        if config.services.file_transfer {
            enabled_services.push("file_transfer".to_string());
        }
        if config.services.messaging {
            enabled_services.push("messaging".to_string());
        }
        if config.services.stream_udp {
            enabled_services.push("stream_udp".to_string());
        }
        if config.services.compute {
            enabled_services.push("compute".to_string());
        }

        // Channel for replaying buffered chunks when a peer becomes trusted
        let (replay_tx, mut replay_rx) =
            tokio::sync::mpsc::unbounded_channel::<([u8; 32], summit_services::BufferedChunk)>();

        let state = summit_api::ApiState {
            sessions: sessions.clone(),
            cache: cache.clone(),
            registry: registry.clone(),
            chunk_tx: chunk_tx.clone(),
            reassembler: reassembler.clone(),
            trust: trust_registry.clone(),
            untrusted_buffer: untrusted_buffer.clone(),
            message_store: message_store.clone(),
            compute_store: compute_store.clone(),
            keypair: keypair.clone(),
            file_transfer_path,
            enabled_services,
            replay_tx,
        };
        tokio::spawn(async move {
            if let Err(e) = summit_api::serve(state, status_port).await {
                tracing::error!(error = %e, "status server failed");
            }
        });

        // Replay task: dispatches buffered chunks from newly-trusted peers
        let replay_dispatcher = dispatcher.clone();
        let replay_reassembler = reassembler.clone();
        tokio::spawn(async move {
            while let Some((peer_pubkey, chunk)) = replay_rx.recv().await {
                tracing::info!(
                    peer = hex::encode(&peer_pubkey[..8]),
                    content_hash = hex::encode(&chunk.content_hash[..8]),
                    type_tag = chunk.type_tag,
                    "replaying buffered chunk from newly-trusted peer"
                );

                // Handle file metadata chunks (type_tag 3)
                if chunk.type_tag == 3 {
                    if let Ok(metadata) =
                        serde_json::from_slice::<summit_services::FileMetadata>(&chunk.payload)
                    {
                        replay_reassembler.add_metadata(metadata).await;
                    }
                }

                // Handle file data chunks (type_tag 2)
                if chunk.type_tag == 2 {
                    if let Ok(Some(path)) = replay_reassembler
                        .add_chunk(chunk.content_hash, chunk.payload.clone())
                        .await
                    {
                        tracing::info!(path = %path.display(), "file completed from replay");
                    }
                }

                // Dispatch to service dispatcher for all other services
                let header = summit_core::wire::ChunkHeader {
                    content_hash: chunk.content_hash,
                    schema_id: chunk.schema_id,
                    type_tag: chunk.type_tag,
                    length: chunk.payload.len() as u32,
                    flags: 0,
                    version: 1,
                };
                replay_dispatcher.dispatch(&peer_pubkey, &header, &chunk.payload);
            }
        })
    };

    // Compute executor
    let _compute_executor = if config.services.compute {
        let store = compute_store.clone();
        let settings = config.services.compute_settings.clone();
        let tx = chunk_tx.clone();
        Some(tokio::spawn(async move {
            summit_services::compute_executor::run(store, settings, tx).await;
        }))
    } else {
        None
    };

    // ── Wait for exit ────────────────────────────────────────────────────────

    let mut shutdown_rx = shutdown_tx.subscribe();

    tokio::select! {
        _ = shutdown_rx.recv()       => tracing::info!("shutting down"),
        r = broadcast_task           => tracing::error!("broadcast task exited: {:?}", r),
        r = listener_task            => tracing::error!("listener task exited: {:?}", r),
        r = expiry_task              => tracing::error!("expiry task exited: {:?}", r),
        r = session_listener_task    => tracing::error!("session listener exited: {:?}", r),
        r = session_initiator_task   => tracing::error!("session initiator exited: {:?}", r),
        r = session_printer          => tracing::error!("session printer exited: {:?}", r),
        r = chunk_manager_task       => tracing::error!("chunk manager exited: {:?}", r),
        r = send_worker_task         => tracing::error!("send worker exited: {:?}", r),
        r = stats_printer            => tracing::error!("stats printer exited: {:?}", r),
    }

    Ok(())
}
