//! Capability announcement broadcast.
//!
//! Periodically sends CapabilityAnnouncement datagrams to the
//! link-local multicast address ff02::1 so nearby peers can
//! discover what this device offers.

use std::net::{Ipv6Addr, SocketAddrV6};
use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context, Result};
use socket2::{Domain, Protocol, Socket, Type};
use tokio::time;
use zerocopy::AsBytes;

use summit_core::wire::{CapabilityAnnouncement, MULTICAST_ADDR, ANNOUNCE_INTERVAL_SECS};

/// Broadcast a set of capability announcements on a regular interval.
///
/// Runs forever — cancel by dropping the task handle.
///
/// # Arguments
/// * `capabilities` - The announcements to broadcast. All are sent each interval.
/// * `interface_index` - The OS interface index to bind to (from `if_nametoindex`).
pub async fn broadcast_loop(
    capabilities: Arc<Vec<CapabilityAnnouncement>>,
    interface_index: u32,
) -> Result<()> {
    let socket = make_multicast_socket(interface_index)
        .context("failed to create multicast broadcast socket")?;

    let multicast: Ipv6Addr = MULTICAST_ADDR.parse().unwrap();
    // Port 0 on the destination — recipients bind to a known port in listener.rs
    let dest = SocketAddrV6::new(multicast, 9000, 0, interface_index);

    let mut interval = time::interval(Duration::from_secs(ANNOUNCE_INTERVAL_SECS));

    tracing::info!(
        interface_index,
        count = capabilities.len(),
        interval_secs = ANNOUNCE_INTERVAL_SECS,
        "capability broadcast starting"
    );

    loop {
        interval.tick().await;

        for cap in capabilities.iter() {
            let bytes = cap.as_bytes();
            match socket.send_to(bytes, &dest.into()) {
                Ok(n) => tracing::trace!(bytes = n, "broadcast sent"),
                Err(e) => tracing::warn!(error = %e, "broadcast send failed"),
            }
        }
    }
}

/// Create a UDP socket suitable for sending IPv6 multicast.
fn make_multicast_socket(interface_index: u32) -> Result<socket2::Socket> {
    let socket = Socket::new(Domain::IPV6, Type::DGRAM, Some(Protocol::UDP))
        .context("socket()")?;

    socket.set_reuse_address(true).context("SO_REUSEADDR")?;
    socket.set_multicast_if_v6(interface_index).context("IPV6_MULTICAST_IF")?;
    // TTL 1 — link-local only, do not route beyond this link
    socket.set_multicast_hops_v6(1).context("IPV6_MULTICAST_HOPS")?;

    Ok(socket)
}

/// Get the OS interface index for a named network interface.
/// Returns an error if the interface does not exist.
pub fn if_index(name: &str) -> Result<u32> {
    let name_cstr = std::ffi::CString::new(name).context("interface name contains null byte")?;
    let index = unsafe { libc::if_nametoindex(name_cstr.as_ptr()) };
    if index == 0 {
        anyhow::bail!("interface '{}' not found", name);
    }
    Ok(index)
}
