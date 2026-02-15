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

/// Receive loop for a single session.
///
/// Listens on the session's socket, decrypts incoming chunks,
/// verifies content hashes, and sends verified chunks to the
/// application via the channel.
pub async fn receive_loop(
    socket:   Arc<UdpSocket>,
    session:  Arc<Mutex<Session>>,
    chunk_tx: mpsc::Sender<IncomingChunk>,
) -> Result<()> {
    let mut buf = vec![0u8; 65536 + 1024]; // max payload + header + MAC overhead

    loop {
        let (len, _peer) = socket.recv_from(&mut buf).await
            .context("recv_from failed")?;

        // Decrypt
        let mut plaintext = Vec::new();
        {
            let mut sess = session.lock().await;
            if let Err(e) = sess.decrypt(&buf[..len], &mut plaintext) {
                tracing::warn!(error = %e, "chunk decryption failed, discarding");
                continue;
            }
        }

        // Parse header
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

        // Verify content hash
        let computed_hash = hash(&payload);
        if computed_hash != header.content_hash {
            tracing::warn!("chunk hash mismatch, discarding");
            continue;
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
            "chunk received"
        );

        // Send to application
        if chunk_tx.send(incoming).await.is_err() {
            bail!("chunk receiver dropped, terminating receive loop");
        }
    }
}
