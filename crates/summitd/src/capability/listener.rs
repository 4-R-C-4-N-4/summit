//! Capability announcement listener.
//!
//! Joins the ff02::1 multicast group and listens for CapabilityAnnouncement
//! datagrams from nearby peers. Valid announcements are upserted into the
//! peer registry. A separate expiry task removes stale entries.

use std::net::{Ipv6Addr, SocketAddrV6};
use std::time::Duration;

use anyhow::{Context, Result};
use socket2::{Domain, Protocol, Socket, Type};
use tokio::net::UdpSocket;
use zerocopy::FromBytes;

use summit_core::wire::{CapabilityAnnouncement, MULTICAST_ADDR_V6, PEER_TTL_SECS};
use summit_services::{PeerEntry, PeerRegistry};

/// UDP port on which capability announcements are received.
pub const ANNOUNCE_PORT: u16 = 9000;

/// Listen for capability announcements and populate the peer registry.
///
/// Runs forever — cancel by dropping the task handle.
pub async fn listener_loop(
    registry: PeerRegistry,
    interface_index: u32,
    local_public_key: [u8; 32],
) -> Result<()> {
    let socket = make_listener_socket(interface_index)
        .context("failed to create multicast listener socket")?;

    // Convert to tokio UdpSocket for async recv
    let socket = UdpSocket::from_std(socket).context("failed to convert to tokio UdpSocket")?;

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

        // Extract the sender's IPv6 address
        let sender_addr = match peer_addr {
            std::net::SocketAddr::V6(v6) => *v6.ip(),
            std::net::SocketAddr::V4(_) => {
                tracing::warn!("received IPv4 datagram on IPv6 socket, ignoring");
                continue;
            }
        };

        // Attempt to parse as a CapabilityAnnouncement
        match CapabilityAnnouncement::read_from_prefix(&buf[..len]) {
            Some(announcement) => {
                // Ignore our own announcements
                if announcement.public_key == local_public_key {
                    tracing::trace!("ignoring own announcement");
                    continue;
                }

                let svc_hash = announcement.service_hash;
                let svc_index = announcement.service_index;
                let svc_count = announcement.service_count;
                let session_port = announcement.session_port;

                tracing::debug!(
                    service_hash = hex::encode(svc_hash),
                    service_index = svc_index,
                    service_count = svc_count,
                    addr = %peer_addr,
                    port = session_port,
                    "service announcement received"
                );

                // Upsert into peer registry — accumulate services.
                registry
                    .entry(announcement.public_key)
                    .and_modify(|entry| {
                        entry.update_from_announcement(&announcement);
                    })
                    .or_insert_with(|| {
                        PeerEntry::from_first_announcement(sender_addr, &announcement)
                    });
            }
            None => {
                tracing::trace!("failed to parse capability announcement");
            }
        }
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
    let socket = Socket::new(Domain::IPV6, Type::DGRAM, Some(Protocol::UDP)).context("socket()")?;

    socket.set_reuse_address(true).context("SO_REUSEADDR")?;
    socket.set_only_v6(true).context("IPV6_V6ONLY")?;
    socket.set_nonblocking(true).context("set_nonblocking")?;

    let bind_addr = SocketAddrV6::new(Ipv6Addr::UNSPECIFIED, ANNOUNCE_PORT, 0, 0);
    socket.bind(&bind_addr.into()).context("bind()")?;

    socket
        .join_multicast_v6(&MULTICAST_ADDR_V6, interface_index)
        .context("IPV6_JOIN_GROUP")?;

    Ok(socket.into())
}
