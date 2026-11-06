//! Platform networking helpers: interface lookup and link-local address.
//!
//! These are the only OS-specific calls on the network path. Everything
//! else goes through std/tokio/socket2, which are portable.

use std::net::{Ipv6Addr, SocketAddrV6};

use anyhow::{Context, Result};

/// Resolve a network interface to its OS index.
///
/// Accepts either an interface name (`eth0`, `wlan0`, `en0`) or a numeric
/// index (`3`). The numeric form is useful on Windows, where interface names
/// are long friendly strings, and when a host app already knows the index.
pub fn if_index(name: &str) -> Result<u32> {
    if let Ok(index) = name.parse::<u32>() {
        if index == 0 {
            anyhow::bail!("interface index 0 is not valid");
        }
        return Ok(index);
    }
    let name_cstr = std::ffi::CString::new(name).context("interface name contains null byte")?;
    let index = name_to_index(&name_cstr);
    if index == 0 {
        anyhow::bail!("interface '{}' not found", name);
    }
    Ok(index)
}

#[cfg(unix)]
fn name_to_index(name: &std::ffi::CStr) -> u32 {
    // Safety: name is a valid NUL-terminated string for the duration of the call.
    unsafe { libc::if_nametoindex(name.as_ptr()) }
}

#[cfg(windows)]
fn name_to_index(name: &std::ffi::CStr) -> u32 {
    #[link(name = "iphlpapi")]
    extern "system" {
        fn if_nametoindex(name: *const std::ffi::c_char) -> u32;
    }
    // Safety: name is a valid NUL-terminated string for the duration of the call.
    unsafe { if_nametoindex(name.as_ptr()) }
}

#[cfg(not(any(unix, windows)))]
fn name_to_index(_name: &std::ffi::CStr) -> u32 {
    0
}

/// Find this host's IPv6 link-local address on the given interface.
///
/// Uses the routing table rather than enumerating addresses: "connecting" a
/// UDP socket to the all-nodes group on the interface makes the kernel pick
/// the source address it would actually use. No packet is sent.
pub fn link_local_addr(interface_index: u32) -> Result<Ipv6Addr> {
    let probe = std::net::UdpSocket::bind("[::]:0").context("bind probe socket")?;
    let dest = SocketAddrV6::new(
        Ipv6Addr::new(0xff02, 0, 0, 0, 0, 0, 0, 1),
        9000,
        0,
        interface_index,
    );
    probe
        .connect(dest)
        .context("no IPv6 route on interface (is IPv6 enabled and the link up?)")?;
    match probe.local_addr()? {
        std::net::SocketAddr::V6(v6) => Ok(*v6.ip()),
        _ => anyhow::bail!("expected IPv6 local address"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn numeric_index_passes_through() {
        assert_eq!(if_index("7").unwrap(), 7);
    }

    #[test]
    fn zero_index_rejected() {
        assert!(if_index("0").is_err());
    }

    #[test]
    fn unknown_name_rejected() {
        assert!(if_index("definitely-not-an-iface").is_err());
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn loopback_resolves() {
        assert!(if_index("lo").unwrap() > 0);
    }
}
