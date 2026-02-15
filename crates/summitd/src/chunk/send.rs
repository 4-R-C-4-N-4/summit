//! Chunk sending — encrypt, frame, transmit.

use std::net::SocketAddr;
use std::sync::Arc;

use anyhow::{Context, Result};
use tokio::net::UdpSocket;
use tokio::sync::Mutex;
use zerocopy::AsBytes;

use summit_core::crypto::{hash, Session};
use summit_core::wire::{ChunkHeader, CHUNK_VERSION};

use super::OutgoingChunk;

/// Send a chunk over an established session.
///
/// Constructs the chunk header, encrypts header + payload via the session's
/// Noise transport state, and transmits the result as a single UDP datagram.
pub async fn send_chunk(
    socket:    Arc<UdpSocket>,
    peer_addr: SocketAddr,
    session:   Arc<Mutex<Session>>,
    chunk:     OutgoingChunk,
) -> Result<()> {
    let content_hash = hash(&chunk.payload);

    let header = ChunkHeader {
        content_hash,
        schema_id: chunk.schema_id,
        type_tag:  chunk.type_tag,
        length:    chunk.payload.len() as u32,
        flags:     0,
        version:   CHUNK_VERSION,
    };

    // Serialize header + payload
    let mut plaintext = Vec::with_capacity(72 + chunk.payload.len());
    plaintext.extend_from_slice(header.as_bytes());
    plaintext.extend_from_slice(&chunk.payload);

    // Encrypt via session
    let mut ciphertext = Vec::new();
    {
        let mut sess = session.lock().await;
        sess.encrypt(&plaintext, &mut ciphertext)
            .context("chunk encryption failed")?;
    }

    // Send encrypted chunk
    socket.send_to(&ciphertext, peer_addr).await
        .context("failed to send chunk")?;

    tracing::info!(
        %peer_addr,
        content_hash = hex::encode(content_hash),
        payload_len = chunk.payload.len(),
        "chunk sent"
    );

    Ok(())
}
