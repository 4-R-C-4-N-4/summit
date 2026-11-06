//! Summit node lifecycle: wire up every subsystem and run until stopped.
//!
//! This is what `summitd` used to do inline in `main`. As a library it has to
//! be well-behaved inside a host process (an Android service, a desktop app):
//! every task it spawns is owned by the [`Node`] and aborted on stop, so the
//! sockets (9000 multicast, API port, session/chunk ports) are released and a
//! new node can be started in the same process.

use std::fmt::Debug;
use std::future::Future;
use std::net::{Ipv6Addr, SocketAddrV6};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context, Result};
use tokio::net::UdpSocket;
use tokio::sync::{broadcast, mpsc};
use tokio::task::JoinSet;

use summit_core::config::{data_dir, SummitConfig};
use summit_core::crypto::Keypair;
use summit_core::wire::{service_hash, Contract};
use summit_services::{
    new_registry, new_session_table, ChunkCache, ChunkService, ComputeService, ComputeStore,
    FileReassembler, KnownSchema, MessageStore, MessagingService, SendTarget, TrustRegistry,
    UntrustedBuffer,
};

use crate::capability::{broadcast as announce, listener};
use crate::dispatch::ServiceDispatcher;
use crate::{chunk, delivery, net, session};

/// Everything needed to start a node. Paths are explicit so a host app (which
/// may have no `$HOME`) decides where state lives.
#[derive(Debug, Clone)]
pub struct NodeOptions {
    /// Interface name (`eth0`, `wlan0`) or numeric index.
    pub interface: String,
    pub config: SummitConfig,
    /// Content-addressed chunk cache directory.
    pub cache_root: PathBuf,
    /// Persisted trust decisions.
    pub trust_path: PathBuf,
}

impl NodeOptions {
    /// Defaults used by the `summitd` binary: XDG/HOME-derived data dir,
    /// `SUMMIT_CACHE` overriding the cache location.
    pub fn from_env(interface: impl Into<String>, config: SummitConfig) -> Self {
        let cache_root = std::env::var("SUMMIT_CACHE")
            .map(PathBuf::from)
            .unwrap_or_else(|_| data_dir().join("cache"));
        Self {
            interface: interface.into(),
            config,
            cache_root,
            trust_path: data_dir().join("trust.json"),
        }
    }
}

/// Cloneable handle that asks a running node to stop. Safe to call from any
/// thread or task, any number of times.
#[derive(Clone)]
pub struct ShutdownHandle(broadcast::Sender<()>);

impl ShutdownHandle {
    pub fn shutdown(&self) {
        let _ = self.0.send(());
    }
}

/// A running node. Drive it with [`Node::run`].
pub struct Node {
    public_key: [u8; 32],
    local_addr: Ipv6Addr,
    interface_index: u32,
    api_port: u16,
    shutdown_tx: broadcast::Sender<()>,
    /// Tasks whose exit means the node is broken.
    critical: JoinSet<(&'static str, String)>,
    /// Supporting tasks that run until the node stops.
    background: JoinSet<()>,
}

impl Node {
    /// Bind sockets, load identity and spawn every subsystem.
    ///
    /// Must be called inside a tokio runtime. Returns once everything is
    /// started; failures to bind or load identity are returned here.
    pub async fn start(opts: NodeOptions) -> Result<Self> {
        let NodeOptions {
            interface,
            config,
            cache_root,
            trust_path,
        } = opts;

        tracing::info!(interface, "summit node starting");

        let interface_index = net::if_index(&interface)?;
        let local_link_addr = wait_for_link_local(interface_index).await?;
        tracing::info!(addr = %local_link_addr, "local link-local address");

        // Bind session socket
        let session_listen_socket = Arc::new(
            UdpSocket::bind(SocketAddrV6::new(local_link_addr, 0, 0, interface_index))
                .await
                .context("failed to bind session listen socket")?,
        );
        let session_listen_port = session_listen_socket.local_addr()?.port();

        let keypair = Arc::new(load_or_create_keypair(&config.identity.keypair_path)?);

        // Shared state
        let registry = new_registry();
        let sessions = new_session_table();
        let handshake_tracker = session::HandshakeTracker::shared();
        let message_store = MessageStore::new();
        let compute_store = ComputeStore::new();

        let cache = ChunkCache::new(&cache_root)?;
        tracing::info!(root = %cache_root.display(), "chunk cache initialized");

        let trust_registry = TrustRegistry::with_persistence(trust_path);
        trust_registry.apply_config(config.trust.auto_trust, &config.trust.trusted_peers);
        if config.trust.auto_trust {
            tracing::warn!("auto-trust enabled — all discovered peers will be trusted");
        }
        let untrusted_buffer = UntrustedBuffer::new();

        // Outbound chunk queue
        let (chunk_tx, chunk_rx) = mpsc::channel::<(SendTarget, chunk::OutgoingChunk)>(256);

        // File reassembler
        let file_transfer_path = config.services.file_transfer_settings.storage_path.clone();
        tracing::info!(path = %file_transfer_path.display(), "file transfer storage path");
        let reassembler = Arc::new(FileReassembler::new(file_transfer_path.clone()));

        // Service dispatcher
        let dispatcher = {
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

        let (broadcast_services, enabled_services) = enabled_services(&config);
        tracing::info!(
            file_transfer = config.services.file_transfer,
            messaging = config.services.messaging,
            stream_udp = config.services.stream_udp,
            compute = config.services.compute,
            "services enabled"
        );

        let (shutdown_tx, _) = broadcast::channel::<()>(1);
        let mut critical = JoinSet::new();
        let mut background = JoinSet::new();

        // ── Discovery ────────────────────────────────────────────────────────
        spawn_critical(
            &mut critical,
            "capability broadcast",
            announce::broadcast_loop(
                keypair.clone(),
                interface_index,
                session_listen_port,
                broadcast_services,
            ),
        );
        spawn_critical(
            &mut critical,
            "capability listener",
            listener::listener_loop(registry.clone(), interface_index, keypair.public),
        );
        spawn_critical(
            &mut critical,
            "registry expiry",
            listener::expiry_loop(registry.clone(), sessions.clone()),
        );

        // ── Sessions ─────────────────────────────────────────────────────────
        spawn_critical(
            &mut critical,
            "session listener",
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
        spawn_critical(
            &mut critical,
            "session initiator",
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
        {
            let sessions = sessions.clone();
            spawn_critical(&mut critical, "session printer", async move {
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
            });
        }

        // ── Data plane ───────────────────────────────────────────────────────
        let delivery_tracker = delivery::DeliveryTracker::new();
        spawn_critical(
            &mut critical,
            "chunk manager",
            chunk::manager::ChunkManager::new(
                sessions.clone(),
                cache.clone(),
                delivery_tracker.clone(),
                reassembler.clone(),
                trust_registry.clone(),
                untrusted_buffer.clone(),
                dispatcher.clone(),
                chunk_tx.clone(),
                shutdown_tx.subscribe(),
                config.network.bulk_rate,
                config.network.bulk_burst,
            )
            .run(),
        );
        spawn_critical(
            &mut critical,
            "send worker",
            chunk::send_worker::SendWorker::new(
                sessions.clone(),
                cache.clone(),
                trust_registry.clone(),
                chunk_rx,
                shutdown_tx.subscribe(),
            )
            .run(),
        );
        spawn_critical(
            &mut critical,
            "recovery",
            chunk::recovery::recovery_loop(
                reassembler.clone(),
                chunk_tx.clone(),
                shutdown_tx.subscribe(),
            ),
        );
        {
            let tracker = delivery_tracker.clone();
            spawn_critical(&mut critical, "stats printer", async move {
                let mut interval = tokio::time::interval(Duration::from_secs(10));
                loop {
                    interval.tick().await;
                    tracker.print_stats();
                    tracker.evict_expired();
                }
            });
        }

        // ── Background services ──────────────────────────────────────────────
        {
            let store = message_store.clone();
            let retention_days = config.services.messaging_settings.retention_days;
            background.spawn(async move {
                let mut interval = tokio::time::interval(Duration::from_secs(3600));
                loop {
                    interval.tick().await;
                    let removed = store.expire(retention_days);
                    if removed > 0 {
                        tracing::info!(removed, retention_days, "expired old messages");
                    }
                }
            });
        }

        // Channel for replaying buffered chunks when a peer becomes trusted
        let (replay_tx, mut replay_rx) =
            mpsc::unbounded_channel::<([u8; 32], summit_services::BufferedChunk)>();

        let api_port = config.network.api_port;
        {
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
                shutdown_tx: shutdown_tx.clone(),
            };
            background.spawn(async move {
                if let Err(e) = summit_api::serve(state, api_port).await {
                    tracing::error!(error = %e, "status server failed");
                }
            });
        }

        // Replay task: dispatches buffered chunks from newly-trusted peers
        {
            let replay_dispatcher = dispatcher.clone();
            background.spawn(async move {
                while let Some((peer_pubkey, chunk)) = replay_rx.recv().await {
                    tracing::info!(
                        peer = hex::encode(&peer_pubkey[..8]),
                        content_hash = hex::encode(&chunk.content_hash[..8]),
                        type_tag = chunk.type_tag,
                        "replaying buffered chunk from newly-trusted peer"
                    );
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
            });
        }

        if config.services.compute {
            let store = compute_store.clone();
            let settings = config.services.compute_settings.clone();
            let tx = chunk_tx.clone();
            let trust = trust_registry.clone();
            background.spawn(async move {
                summit_services::compute_executor::run(store, settings, tx, trust).await;
            });
        }

        Ok(Self {
            public_key: keypair.public,
            local_addr: local_link_addr,
            interface_index,
            api_port,
            shutdown_tx,
            critical,
            background,
        })
    }

    pub fn public_key(&self) -> [u8; 32] {
        self.public_key
    }

    pub fn local_addr(&self) -> Ipv6Addr {
        self.local_addr
    }

    pub fn interface_index(&self) -> u32 {
        self.interface_index
    }

    /// Port of the local REST API (127.0.0.1).
    pub fn api_port(&self) -> u16 {
        self.api_port
    }

    pub fn shutdown_handle(&self) -> ShutdownHandle {
        ShutdownHandle(self.shutdown_tx.clone())
    }

    /// Run until shutdown is requested (handle, `/api/daemon/shutdown`) or a
    /// critical task dies. Then abort everything and wait for it, so all
    /// sockets are closed when this returns.
    pub async fn run(mut self) -> Result<()> {
        let mut shutdown_rx = self.shutdown_tx.subscribe();
        let outcome = tokio::select! {
            biased;
            _ = shutdown_rx.recv() => {
                tracing::info!("shutting down");
                Ok(())
            }
            Some(res) = self.critical.join_next() => {
                let msg = match res {
                    Ok((name, result)) => format!("{name} exited: {result}"),
                    Err(e) => format!("task failed: {e}"),
                };
                // Tasks that watch the shutdown channel return as soon as it
                // fires, possibly before we observe it ourselves.
                if shutdown_rx.try_recv().is_ok() {
                    tracing::info!("shutting down");
                    Ok(())
                } else {
                    tracing::error!("{msg}");
                    Err(anyhow::anyhow!(msg))
                }
            }
        };
        // Let tasks that watch the shutdown channel see it, then reap all.
        let _ = self.shutdown_tx.send(());
        self.critical.shutdown().await;
        self.background.shutdown().await;
        outcome
    }
}

/// How long to wait for a usable link-local address at startup.
const LINK_LOCAL_WAIT: Duration = Duration::from_secs(15);

/// A link that just came up (boot, Wi-Fi reconnect, USB tether plugged in)
/// has no usable link-local address until IPv6 duplicate address detection
/// finishes, typically 1–2 s. Wait for it instead of failing the start.
async fn wait_for_link_local(interface_index: u32) -> Result<Ipv6Addr> {
    let deadline = tokio::time::Instant::now() + LINK_LOCAL_WAIT;
    let mut logged = false;
    loop {
        match net::link_local_addr(interface_index) {
            Ok(addr) => return Ok(addr),
            Err(e) if tokio::time::Instant::now() < deadline => {
                if !logged {
                    tracing::info!(error = %e, "waiting for link-local address");
                    logged = true;
                }
                tokio::time::sleep(Duration::from_millis(250)).await;
            }
            Err(e) => return Err(e),
        }
    }
}

fn spawn_critical<F, T>(set: &mut JoinSet<(&'static str, String)>, name: &'static str, fut: F)
where
    F: Future<Output = T> + Send + 'static,
    T: Debug + Send + 'static,
{
    set.spawn(async move {
        let out = fut.await;
        (name, format!("{out:?}"))
    });
}

/// Services to announce on the wire, and their names for the API.
fn enabled_services(config: &SummitConfig) -> (Vec<announce::ServiceEntry>, Vec<String>) {
    let s = &config.services;
    let table = [
        (s.file_transfer, "file_transfer", Contract::Bulk, 0),
        (s.messaging, "messaging", Contract::Bulk, 0),
        (
            s.stream_udp,
            "stream_udp",
            Contract::Realtime,
            config.network.chunk_port,
        ),
        (s.compute, "compute", Contract::Bulk, 0),
    ];
    let mut entries = Vec::new();
    let mut names = Vec::new();
    for (enabled, name, contract, chunk_port) in table {
        if enabled {
            entries.push(announce::ServiceEntry {
                hash: service_hash(format!("summit.{name}").as_bytes()),
                contract,
                chunk_port,
            });
            names.push(name.to_string());
        }
    }
    (entries, names)
}

/// Load the X25519 identity key, or generate and persist one (mode 0600).
pub fn load_or_create_keypair(path: &Path) -> Result<Keypair> {
    if path.exists() {
        let bytes = std::fs::read(path)
            .with_context(|| format!("failed to read keypair from {}", path.display()))?;
        let key: [u8; 32] = bytes.as_slice().try_into().map_err(|_| {
            anyhow::anyhow!(
                "keypair file {} has wrong size ({} bytes, expected 32)",
                path.display(),
                bytes.len()
            )
        })?;
        let kp = Keypair::from_private(key);
        tracing::info!(
            public_key = hex::encode(kp.public),
            path = %path.display(),
            "loaded keypair from disk"
        );
        return Ok(kp);
    }

    let kp = Keypair::generate();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).ok();
    }
    std::fs::write(path, *kp.private_bytes())
        .with_context(|| format!("failed to save keypair to {}", path.display()))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600)).ok();
    }
    tracing::info!(
        public_key = hex::encode(kp.public),
        path = %path.display(),
        "generated and saved new keypair"
    );
    Ok(kp)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keypair_roundtrips_through_disk() {
        let dir = std::env::temp_dir().join(format!("summit-kp-test-{}", std::process::id()));
        let path = dir.join("nested").join("keypair");
        let a = load_or_create_keypair(&path).unwrap();
        let b = load_or_create_keypair(&path).unwrap();
        assert_eq!(a.public, b.public);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn keypair_wrong_size_rejected() {
        let dir = std::env::temp_dir().join(format!("summit-kp-bad-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("keypair");
        std::fs::write(&path, [1u8; 7]).unwrap();
        assert!(load_or_create_keypair(&path).is_err());
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn enabled_services_match_config() {
        let mut config = SummitConfig::default();
        config.services.file_transfer = true;
        config.services.messaging = false;
        config.services.stream_udp = false;
        config.services.compute = true;
        let (entries, names) = enabled_services(&config);
        assert_eq!(names, ["file_transfer", "compute"]);
        assert_eq!(entries[0].hash, service_hash(b"summit.file_transfer"));
        assert_eq!(entries[1].hash, service_hash(b"summit.compute"));
    }
}
