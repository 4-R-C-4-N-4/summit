// //! Noise_XX handshake over UDP.
//
// use std::net::{SocketAddr, Ipv6Addr, SocketAddrV6};
// use std::sync::Arc;
// use std::time::Duration;
//
// use anyhow::{bail, Context, Result};
// use tokio::net::UdpSocket;
// use tokio::sync::Mutex;
// use tokio::time::timeout;
// use zerocopy::{AsBytes, FromBytes};
//
// use summit_core::crypto::{Keypair, NoiseInitiator, NoiseResponder};
// use summit_core::wire::{
//     Contract, HandshakeComplete, HandshakeInit, HandshakeResponse,
//     CapabilityAnnouncement, HANDSHAKE_TIMEOUT_SECS,
// };
//
// use super::{ActiveSession, SessionMeta, SessionTable};
//
// const MSG_TIMEOUT: Duration = Duration::from_secs(HANDSHAKE_TIMEOUT_SECS);
//
// // ── Initiator ─────────────────────────────────────────────────────────────────
// pub async fn initiate(
//     socket:          Arc<UdpSocket>,
//     peer_addr:       SocketAddr,
//     announcement:    &CapabilityAnnouncement,
//     local_keypair:   &Keypair,
//     sessions:        SessionTable,
//     contract:        Contract,
//     interface_index: u32,
// ) -> Result<[u8; 32]> {
//     tracing::debug!(peer_addr = %peer_addr, "initiating handshake");
//
//     // Create dedicated chunk socket
//     let chunk_socket = UdpSocket::bind(SocketAddrV6::new(
//         Ipv6Addr::UNSPECIFIED,
//         0,
//         0,
//         interface_index,
//     )).await.context("failed to bind chunk socket")?;
//     let local_chunk_port = chunk_socket.local_addr()?.port();
//
//     // Create Noise initiator
//     let (noise, msg1) = NoiseInitiator::new(local_keypair)
//     .context("failed to create noise initiator")?;
//
//     // Build HandshakeInit with msg1 and nonce
//     let init = HandshakeInit {
//         noise_msg: msg1.try_into().map_err(|_| anyhow::anyhow!("msg1 wrong size"))?,
//         nonce: *noise.nonce(),
//         capability_hash: announcement.capability_hash,
//     };
//
//     // Send HandshakeInit
//     socket.send_to(init.as_bytes(), peer_addr).await
//     .context("failed to send HandshakeInit")?;
//
//     // Receive HandshakeResponse
//     let mut buf = vec![0u8; 512];
//     let (len, recv_addr) = timeout(
//         MSG_TIMEOUT,
//         socket.recv_from(&mut buf)
//     ).await
//     .context("timeout waiting for HandshakeResponse")?
//     .context("recv_from failed")?;
//
//     let response = HandshakeResponse::read_from_prefix(&buf[..len])
//     .context("failed to parse HandshakeResponse")?;
//
//     // Finish handshake and get Session + msg3
//     let (mut session, msg3) = noise.finish(&response.noise_msg, &response.nonce)
//     .context("failed to finish handshake")?;
//
//     // Build and send HandshakeComplete
//     let complete = HandshakeComplete {
//         noise_msg: msg3.try_into().map_err(|_| anyhow::anyhow!("msg3 wrong size"))?,
//     };
//
//     socket.send_to(complete.as_bytes(), peer_addr).await
//     .context("failed to send HandshakeComplete")?;
//
//     let session_id = session.session_id;
//
//     // Send our chunk_port (encrypted)
//     let chunk_port_msg = local_chunk_port.to_le_bytes();
//     let mut encrypted = Vec::new();
//     session.encrypt(&chunk_port_msg, &mut encrypted)
//     .context("failed to encrypt chunk_port")?;
//     socket.send_to(&encrypted, peer_addr).await
//     .context("failed to send chunk_port")?;
//
//     // Receive peer's chunk_port
//     let (len, _) = timeout(
//         MSG_TIMEOUT,
//         socket.recv_from(&mut buf)
//     ).await
//     .context("timeout waiting for peer chunk_port")?
//     .context("recv_from failed")?;
//
//     let mut decrypted = Vec::new();
//     session.decrypt(&buf[..len], &mut decrypted)
//     .context("failed to decrypt peer chunk_port")?;
//
//     let peer_chunk_port = u16::from_le_bytes([decrypted[0], decrypted[1]]);
//
//     // Insert session
//     sessions.insert(session_id, ActiveSession {
//         meta: SessionMeta {
//             session_id,
//             peer_addr,
//             chunk_port: peer_chunk_port,
//             contract,
//             established_at: std::time::Instant::now(),
//         },
//         crypto: Arc::new(Mutex::new(session)),
//                     socket: Arc::new(chunk_socket),
//     });
//
//     tracing::info!(
//         peer_addr = %peer_addr,
//         session_id = hex::encode(session_id),
//                    peer_chunk_port,
//                    "session established (initiator)"
//     );
//
//     Ok(session_id)
// }
//
// // ── Responder ─────────────────────────────────────────────────────────────────
// pub async fn respond(
//     socket:          Arc<UdpSocket>,
//     peer_addr:       SocketAddr,
//     init_bytes:      &[u8],
//     local_keypair:   &Keypair,
//     sessions:        SessionTable,
//     contract:        Contract,
//     interface_index: u32,
// ) -> Result<[u8; 32]> {
//     tracing::debug!(peer_addr = %peer_addr, "responding to handshake");
//
//     // Create dedicated chunk socket
//     let chunk_socket = UdpSocket::bind(SocketAddrV6::new(
//         Ipv6Addr::UNSPECIFIED,
//         0,
//         0,
//         interface_index,
//     )).await.context("failed to bind chunk socket")?;
//     let local_chunk_port = chunk_socket.local_addr()?.port();
//
//     // // Create ephemeral socket for handshake
//     // let socket = UdpSocket::bind(SocketAddrV6::new(
//     //     Ipv6Addr::UNSPECIFIED,
//     //     0,
//     //     0,
//     //     interface_index,
//     // )).await.context("failed to bind handshake socket")?;
//
//     // Parse HandshakeInit
//     let init = HandshakeInit::read_from_prefix(init_bytes)
//     .context("failed to parse HandshakeInit")?;
//
//     // Create Noise responder
//     let noise = NoiseResponder::new(local_keypair)
//     .context("failed to create noise responder")?;
//
//     let responder_nonce = *noise.nonce();  // Save it before consuming noise
//
//     // Process msg1 and create msg2
//     let (pending, msg2) = noise.respond(&init.noise_msg, &init.nonce)
//     .context("failed to respond to handshake")?;
//
//     // Build and send HandshakeResponse
//     let response = HandshakeResponse {
//         noise_msg: msg2.try_into().map_err(|_| anyhow::anyhow!("msg2 wrong size"))?,
//         nonce: responder_nonce,  // Use the saved nonce
//     };
//
//     socket.send_to(response.as_bytes(), peer_addr).await
//     .context("failed to send HandshakeResponse")?;
//
//     // Receive HandshakeComplete
//     let mut buf = vec![0u8; 512];
//     let (len, recv_addr) = timeout(
//         MSG_TIMEOUT,
//         socket.recv_from(&mut buf)
//     ).await
//     .context("timeout waiting for HandshakeComplete")?
//     .context("recv_from failed")?;
//
//     let complete = HandshakeComplete::read_from_prefix(&buf[..len])
//     .context("failed to parse HandshakeComplete")?;
//
//     // Finish handshake
//     let mut session = pending.finish(&complete.noise_msg)
//     .context("failed to finish handshake")?;
//
//     let session_id = session.session_id;
//
//     // Receive initiator's chunk_port
//     let (len, _) = timeout(
//         MSG_TIMEOUT,
//         socket.recv_from(&mut buf)
//     ).await
//     .context("timeout waiting for initiator chunk_port")?
//     .context("recv_from failed")?;
//
//     let mut decrypted = Vec::new();
//     session.decrypt(&buf[..len], &mut decrypted)
//     .context("failed to decrypt initiator chunk_port")?;
//
//     let peer_chunk_port = u16::from_le_bytes([decrypted[0], decrypted[1]]);
//
//     // Send our chunk_port back
//     let chunk_port_msg = local_chunk_port.to_le_bytes();
//     let mut encrypted = Vec::new();
//     session.encrypt(&chunk_port_msg, &mut encrypted)
//     .context("failed to encrypt chunk_port")?;
//     socket.send_to(&encrypted, peer_addr).await
//     .context("failed to send chunk_port")?;
//
//     // Insert session
//     sessions.insert(session_id, ActiveSession {
//         meta: SessionMeta {
//             session_id,
//             peer_addr,
//             chunk_port: peer_chunk_port,
//             contract,
//             established_at: std::time::Instant::now(),
//         },
//         crypto: Arc::new(Mutex::new(session)),
//                     socket: Arc::new(chunk_socket),
//     });
//
//     tracing::info!(
//         peer_addr = %peer_addr,
//         session_id = hex::encode(session_id),
//                    peer_chunk_port,
//                    "session established (responder)"
//     );
//
//     Ok(session_id)
// }
