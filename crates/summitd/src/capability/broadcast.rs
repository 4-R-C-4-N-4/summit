//! Capability announcement broadcast.
//!
//! Periodically sends one CapabilityAnnouncement datagram per enabled service
//! to the link-local multicast address ff02::1. Receivers accumulate by
//! public_key to build each peer's full service set.

use std::net::{Ipv6Addr, SocketAddrV6};
use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context, Result};
use socket2::{Domain, Protocol, Socket, Type};
use zerocopy::AsBytes;

use summit_core::crypto::Keypair;
use summit_core::wire::{CapabilityAnnouncement, Contract, ServiceHash, MULTICAST_ADDR};

/// One service to announce, with its contract and optional dedicated port.
#[derive(Debug, Clone)]
pub struct ServiceEntry {
    pub hash: ServiceHash,
    pub contract: Contract,
    /// 0 means "use session_port" (typical for Bulk services).
    pub chunk_port: u16,
}

/// Broadcast all enabled services on a regular interval.
///
/// Sends one datagram per service per tick. Cancel by dropping the task handle.
///
/// # Arguments
/// * `keypair` — This node's identity keypair. Public key goes in each datagram.
/// * `interface_index` — OS interface index to bind to.
/// * `session_port` — TCP port for session handshakes.
/// * `services` — List of services to announce. Built from config.
pub async fn broadcast_loop(
    keypair: Arc<Keypair>,
    interface_index: u32,
    session_port: u16,
    services: Vec<ServiceEntry>,
) -> Result<()> {
    let socket = make_multicast_socket(interface_index)
        .context("failed to create multicast broadcast socket")?;

    let interval_secs = summit_core::wire::ANNOUNCE_INTERVAL_SECS;
    let mut interval = tokio::time::interval(Duration::from_secs(interval_secs));

    let multicast: Ipv6Addr = MULTICAST_ADDR.parse().unwrap();
    let dest = SocketAddrV6::new(multicast, 9000, 0, interface_index);

    let service_count = services.len() as u8;

    tracing::info!(
        interface_index,
        service_count,
        interval_secs,
        "capability broadcast starting"
    );

    loop {
        interval.tick().await;

        for (index, entry) in services.iter().enumerate() {
            let announcement = CapabilityAnnouncement {
                service_hash: entry.hash,
                public_key: keypair.public,
                version: 1,
                session_port,
                chunk_port: entry.chunk_port,
                contract: entry.contract as u8,
                flags: 0,
                service_count,
                service_index: index as u8,
            };

            let bytes = announcement.as_bytes();

            match socket.send_to(bytes, &dest.into()) {
                Ok(n) => tracing::trace!(
                    service_index = index,
                    bytes = n,
                    "broadcast sent"
                ),
                Err(e) => tracing::warn!(
                    service_index = index,
                    error = %e,
                    "broadcast send failed"
                ),
            }
        }
    }
}

/// Create a UDP socket suitable for sending IPv6 multicast.
fn make_multicast_socket(interface_index: u32) -> Result<socket2::Socket> {
    let socket =
        Socket::new(Domain::IPV6, Type::DGRAM, Some(Protocol::UDP)).context("socket()")?;
    socket.set_reuse_address(true).context("SO_REUSEADDR")?;
    socket
        .set_multicast_if_v6(interface_index)
        .context("IPV6_MULTICAST_IF")?;
    socket
        .set_multicast_hops_v6(1)
        .context("IPV6_MULTICAST_HOPS")?;
    Ok(socket)
}

/// Get the OS interface index for a named network interface.
pub fn if_index(name: &str) -> Result<u32> {
    let name_cstr =
        std::ffi::CString::new(name).context("interface name contains null byte")?;
    let index = unsafe { libc::if_nametoindex(name_cstr.as_ptr()) };
    if index == 0 {
        anyhow::bail!("interface '{}' not found", name);
    }
    Ok(index)
}
