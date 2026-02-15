//! Noise_XX handshake over UDP.

use std::net::{SocketAddr, Ipv6Addr, SocketAddrV6};
use std::sync::Arc;
use std::time::Duration;

use anyhow::{bail, Context, Result};
use tokio::net::UdpSocket;
use tokio::sync::Mutex;
use tokio::time::timeout;
use zerocopy::{AsBytes, FromBytes};

use summit_core::crypto::{Keypair, NoiseInitiator, NoiseResponder};
use summit_core::wire::{
    Contract, HandshakeComplete, HandshakeInit, HandshakeResponse,
    CapabilityAnnouncement, HANDSHAKE_TIMEOUT_SECS,
};

use super::{ActiveSession, SessionMeta, SessionTable};

const MSG_TIMEOUT: Duration = Duration::from_secs(HANDSHAKE_TIMEOUT_SECS);

// ── Initiator ─────────────────────────────────────────────────────────────────

pub async fn initiate(
    socket:          Arc<UdpSocket>,
    peer_addr:       SocketAddr,
    announcement:    &CapabilityAnnouncement,
    local_keypair:   &Keypair,
    sessions:        SessionTable,
    contract:        Contract,
    interface_index: u32,
) -> Result<[u8; 32]> {
    tracing::debug!(%peer_addr, "initiating handshake");

    let (initiator, msg1_payload) = NoiseInitiator::new(local_keypair)
    .context("failed to create Noise initiator")?;
    let initiator_nonce = *initiator.nonce();

    let mut init = HandshakeInit {
        nonce:           initiator_nonce,
        capability_hash: announcement.capability_hash,
        noise_msg:       [0u8; 32],
    };
    let copy_len = msg1_payload.len().min(32);
    init.noise_msg[..copy_len].copy_from_slice(&msg1_payload[..copy_len]);

    socket.send_to(init.as_bytes(), peer_addr).await
    .context("failed to send HandshakeInit")?;
    tracing::trace!(%peer_addr, "sent HandshakeInit (msg1)");

    // Wait for msg2
    let mut buf = vec![0u8; 512];
    let (len, from) = timeout(MSG_TIMEOUT, socket.recv_from(&mut buf))
    .await
    .context("timeout waiting for HandshakeResponse")?
    .context("recv HandshakeResponse failed")?;

    // Validate IP matches
    let expected_ip = match peer_addr {
        SocketAddr::V6(addr) => *addr.ip(),
        _ => bail!("expected IPv6 peer address"),
    };
    let from_ip = match from {
        SocketAddr::V6(addr) => *addr.ip(),
        _ => bail!("received response from non-IPv6 address"),
    };
    if from_ip != expected_ip {
        bail!("HandshakeResponse from unexpected address: {from}");
    }

    let response = HandshakeResponse::read_from_prefix(&buf[..len])
    .context("failed to parse HandshakeResponse")?;

    tracing::trace!(from = %from, "received HandshakeResponse (msg2)");

    let responder_nonce = response.nonce;
    let (session, msg3_bytes) = initiator
    .finish(&response.noise_msg, &responder_nonce)
    .context("Noise handshake finish failed")?;

    let session_id = session.session_id;

    let mut complete = HandshakeComplete { noise_msg: [0u8; 64] };
    let copy_len = msg3_bytes.len().min(64);
    complete.noise_msg[..copy_len].copy_from_slice(&msg3_bytes[..copy_len]);

    // Send msg3 to the SOURCE of msg2, not the original peer_addr
    socket.send_to(complete.as_bytes(), from).await
    .context("failed to send HandshakeComplete")?;
    tracing::trace!(to = %from, session_id = hex::encode(session_id), "sent HandshakeComplete (msg3)");

    // Create dedicated socket for chunk I/O on this session
    let chunk_socket = UdpSocket::bind(SocketAddrV6::new(
        Ipv6Addr::UNSPECIFIED,
        9002,  // fixed port instead of 0
        0,
        interface_index,
    )).await.context("failed to bind chunk socket")?;

    sessions.insert(session_id, ActiveSession {
        meta: SessionMeta {
            session_id,
            peer_addr,
            contract,
            established_at: std::time::Instant::now(),
        },
        crypto: Arc::new(Mutex::new(session)),
                    socket: Arc::new(chunk_socket),
    });

    tracing::info!(%peer_addr, session_id = hex::encode(session_id), "session established (initiator)");
    Ok(session_id)
}

// ── Responder ─────────────────────────────────────────────────────────────────

pub async fn respond(
    peer_addr:       SocketAddr,
    init_bytes:      &[u8],
    local_keypair:   &Keypair,
    sessions:        SessionTable,
    contract:        Contract,
    interface_index: u32,
) -> Result<[u8; 32]> {
    tracing::debug!(%peer_addr, "responding to handshake");

    let response_socket = UdpSocket::bind(SocketAddrV6::new(
        Ipv6Addr::UNSPECIFIED,
        0,
        0,
        interface_index,
    )).await.context("failed to bind response socket")?;

    let init = HandshakeInit::read_from_prefix(init_bytes)
    .context("failed to parse HandshakeInit")?;

    let initiator_nonce = init.nonce;

    let responder = NoiseResponder::new(local_keypair)
    .context("failed to create Noise responder")?;
    let responder_nonce = *responder.nonce();

    let (pending, msg2_bytes) = responder
    .respond(&init.noise_msg, &initiator_nonce)
    .context("Noise respond failed")?;

    let mut response = HandshakeResponse {
        nonce:     responder_nonce,
        noise_msg: [0u8; 96],
    };
    response.noise_msg.copy_from_slice(&msg2_bytes);

    response_socket.send_to(response.as_bytes(), peer_addr).await
    .context("failed to send HandshakeResponse")?;
    tracing::trace!(%peer_addr, "sent HandshakeResponse (msg2)");

    // Wait for msg3 on OUR socket
    let mut buf = vec![0u8; 256];
    let (len, from) = timeout(MSG_TIMEOUT, response_socket.recv_from(&mut buf))
    .await
    .context("timeout waiting for HandshakeComplete")?
    .context("recv HandshakeComplete failed")?;

    if from != peer_addr {
        bail!("HandshakeComplete from unexpected address: {from}");
    }

    let complete = HandshakeComplete::read_from_prefix(&buf[..len])
    .context("failed to parse HandshakeComplete")?;

    let session = pending
    .finish(&complete.noise_msg)
    .context("Noise handshake finish (responder) failed")?;

    let session_id = session.session_id;

    // Create dedicated socket for chunk I/O on this session
    let chunk_socket = UdpSocket::bind(SocketAddrV6::new(
        Ipv6Addr::UNSPECIFIED,
        9002,  // fixed port instead of 0
        0,
        interface_index,
    )).await.context("failed to bind chunk socket")?;

    sessions.insert(session_id, ActiveSession {
        meta: SessionMeta {
            session_id,
            peer_addr,
            contract,
            established_at: std::time::Instant::now(),
        },
        crypto: Arc::new(Mutex::new(session)),
                    socket: Arc::new(chunk_socket),
    });

    tracing::info!(%peer_addr, session_id = hex::encode(session_id), "session established (responder)");
    Ok(session_id)
}
