//! Chunk receiving — decrypt, verify, deliver.

use std::sync::Arc;

use anyhow::{bail, Context, Result};
use bytes::Bytes;
use tokio::net::UdpSocket;
use tokio::sync::{mpsc, Mutex};
use zerocopy::FromBytes;

use summit_core::crypto::{hash, Session};
use summit_core::wire::ChunkHeader;

use super::IncomingChunk;

use crate::cache::ChunkCache;

pub async fn receive_loop(
    socket:   Arc<UdpSocket>,
    session:  Arc<Mutex<Session>>,
    chunk_tx: mpsc::Sender<IncomingChunk>,
    cache:    ChunkCache,  // NEW
) -> Result<()> {
    let mut buf = vec![0u8; 65536 + 1024];

    loop {
        let (len, _peer) = socket.recv_from(&mut buf).await
        .context("recv_from failed")?;

        let mut plaintext = Vec::new();
        {
            let mut sess = session.lock().await;
            if let Err(e) = sess.decrypt(&buf[..len], &mut plaintext) {
                tracing::warn!(error = %e, "chunk decryption failed, discarding");
                continue;
            }
        }

        if plaintext.len() < 72 {
            tracing::trace!("received chunk too short, discarding");
            continue;
        }

        let header = match ChunkHeader::read_from_prefix(&plaintext[..72]) {
            Some(h) => h,
            None => {
                tracing::trace!("failed to parse chunk header, discarding");
                continue;
            }
        };

        let payload = Bytes::copy_from_slice(&plaintext[72..]);

        let computed_hash = hash(&payload);
        if computed_hash != header.content_hash {
            tracing::warn!("chunk hash mismatch, discarding");
            continue;
        }

        // Cache the chunk
        if let Err(e) = cache.put(&header.content_hash, &payload) {
            tracing::warn!(error = %e, "failed to cache chunk");
            // Continue anyway — caching is best-effort
        }

        let incoming = IncomingChunk {
            content_hash: header.content_hash,
            type_tag:     header.type_tag,
            schema_id:    header.schema_id,
            payload,
        };

        tracing::info!(
            content_hash = hex::encode(incoming.content_hash),
                       type_tag = incoming.type_tag,
                       payload_len = incoming.payload.len(),
                       cached = true,
                       "chunk received"
        );

        if chunk_tx.send(incoming).await.is_err() {
            bail!("chunk receiver dropped, terminating receive loop");
        }
    }
}
