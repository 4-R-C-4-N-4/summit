//! Capability announcement listener.
//!
//! Joins the ff02::1 multicast group and listens for CapabilityAnnouncement
//! datagrams from nearby peers. Valid announcements are upserted into the
//! peer registry. A separate expiry task removes stale entries.

use std::net::{Ipv6Addr, SocketAddrV6};
use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use socket2::{Domain, Protocol, Socket, Type};
use tokio::net::UdpSocket;
use zerocopy::FromBytes;

use summit_core::wire::{
    CapabilityAnnouncement, Contract, MULTICAST_ADDR, PEER_TTL_SECS,
};

use super::{PeerEntry, PeerRegistry};

/// UDP port on which capability announcements are received.
pub const ANNOUNCE_PORT: u16 = 9000;

/// Listen for capability announcements and populate the peer registry.
///
/// Runs forever — cancel by dropping the task handle.
pub async fn listener_loop(
    registry: PeerRegistry,
    interface_index: u32,
) -> Result<()> {
    let socket = make_listener_socket(interface_index)
        .context("failed to create multicast listener socket")?;

    // Convert to tokio UdpSocket for async recv
    let socket = UdpSocket::from_std(socket.into())
        .context("failed to convert to tokio UdpSocket")?;

    let mut buf = vec![0u8; 1024];

    tracing::info!(port = ANNOUNCE_PORT, "capability listener starting");

    loop {
        let (len, peer_addr) = match socket.recv_from(&mut buf).await {
            Ok(r) => r,
            Err(e) => {
                tracing::warn!(error = %e, "recv_from failed");
                continue;
            }
        };

        let data = &buf[..len];

        // Attempt to parse as a CapabilityAnnouncement
        let announcement = match CapabilityAnnouncement::read_from_prefix(data) {
            Some(a) => a,
            None => {
                tracing::trace!(len, "received datagram too short to parse, ignoring");
                continue;
            }
        };

        // Extract the sender's IPv6 address
        let sender_addr = match peer_addr {
            std::net::SocketAddr::V6(v6) => *v6.ip(),
            std::net::SocketAddr::V4(_) => {
                tracing::warn!("received IPv4 datagram on IPv6 socket, ignoring");
                continue;
            }
        };

        // Validate contract byte
        let contract = match Contract::try_from(announcement.contract) {
            Ok(c) => c,
            Err(e) => {
                tracing::trace!(error = %e, "invalid contract byte, ignoring");
                continue;
            }
        };

        let entry = PeerEntry {
            addr:         sender_addr,
            public_key:   announcement.public_key,
            session_port: u16::from_ne_bytes(announcement.session_port.to_ne_bytes()),
            version:      u32::from_ne_bytes(announcement.version.to_ne_bytes()),
            contract,
            last_seen:    Instant::now(),
        };

        tracing::debug!(
            capability = hex::encode(announcement.capability_hash),
            addr = %sender_addr,
            port = entry.session_port,
            "peer discovered"
        );

        registry.insert(announcement.capability_hash, entry);
    }
}

/// Remove registry entries that have not been refreshed within the TTL.
///
/// Runs forever — cancel by dropping the task handle.
pub async fn expiry_loop(registry: PeerRegistry) -> Result<()> {
    let ttl = Duration::from_secs(PEER_TTL_SECS);
    let check_interval = Duration::from_secs(1);
    let mut interval = tokio::time::interval(check_interval);

    loop {
        interval.tick().await;

        let before = registry.len();
        registry.retain(|_, entry| entry.last_seen.elapsed() < ttl);
        let after = registry.len();

        if before != after {
            tracing::debug!(removed = before - after, "expired peer registry entries");
        }
    }
}

/// Create a UDP socket joined to the ff02::1 multicast group.
fn make_listener_socket(interface_index: u32) -> Result<std::net::UdpSocket> {
    let socket = Socket::new(Domain::IPV6, Type::DGRAM, Some(Protocol::UDP))
    .context("socket()")?;

    socket.set_reuse_address(true).context("SO_REUSEADDR")?;
    socket.set_only_v6(true).context("IPV6_V6ONLY")?;
    socket.set_nonblocking(true).context("set_nonblocking")?;  // add this line

    let bind_addr = SocketAddrV6::new(Ipv6Addr::UNSPECIFIED, ANNOUNCE_PORT, 0, 0);
    socket.bind(&bind_addr.into()).context("bind()")?;

    let multicast: Ipv6Addr = MULTICAST_ADDR.parse().unwrap();
    socket
    .join_multicast_v6(&multicast, interface_index)
    .context("IPV6_JOIN_GROUP")?;

    Ok(socket.into())
}
