//! Chunk sending — encrypt, frame, transmit.

use std::net::SocketAddr;
use std::sync::Arc;

use anyhow::{Context, Result};
use tokio::net::UdpSocket;
use tokio::sync::Mutex;
use zerocopy::AsBytes;

use summit_core::crypto::{hash, Session};
use summit_core::wire::{ChunkHeader, CHUNK_VERSION};
use summit_services::ChunkCache;

use super::OutgoingChunk;

pub async fn send_chunk(
    socket: Arc<UdpSocket>,
    peer_addr: SocketAddr,
    session: Arc<Mutex<Session>>,
    chunk: OutgoingChunk,
    cache: ChunkCache,
) -> Result<()> {
    let content_hash = hash(&chunk.payload);

    // Store in cache before sending (dedup for future sends)
    cache
        .put(&content_hash, &chunk.payload)
        .context("failed to cache chunk")?;

    let header = ChunkHeader {
        content_hash,
        schema_id: chunk.schema_id,
        type_tag: chunk.type_tag,
        length: chunk.payload.len() as u32,
        flags: 0,
        version: CHUNK_VERSION,
    };

    let mut plaintext = Vec::with_capacity(72 + chunk.payload.len());
    plaintext.extend_from_slice(header.as_bytes());
    plaintext.extend_from_slice(&chunk.payload);

    let mut ciphertext = Vec::new();
    {
        let mut sess = session.lock().await;
        sess.encrypt(&plaintext, &mut ciphertext)
            .context("chunk encryption failed")?;
    }

    socket
        .send_to(&ciphertext, peer_addr)
        .await
        .context("failed to send chunk")?;

    tracing::info!(
        %peer_addr,
        content_hash = hex::encode(content_hash),
                   payload_len = chunk.payload.len(),
                   cached = true,
                   "chunk sent"
    );

    Ok(())
}
