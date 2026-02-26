use std::sync::Arc;
use std::time::Duration;

use anyhow::{bail, Context, Result};
use bytes::Bytes;
use tokio::net::UdpSocket;
use tokio::sync::{mpsc, Mutex};
use zerocopy::FromBytes;

use summit_core::crypto::{hash, Session};
use summit_core::wire::ChunkHeader;
use summit_services::{ChunkCache, KnownSchema};

/// How long to wait for data before considering the session dead.
const RECEIVE_TIMEOUT: Duration = Duration::from_secs(60);

use super::IncomingChunk;

use crate::delivery::DeliveryTracker;
use crate::dispatch::ServiceDispatcher;

pub async fn receive_loop(
    socket: Arc<UdpSocket>,
    session: Arc<Mutex<Session>>,
    chunk_tx: mpsc::Sender<IncomingChunk>,
    cache: ChunkCache,
    tracker: DeliveryTracker,
    peer_addr: String,
    dispatcher: Arc<ServiceDispatcher>,
    peer_pubkey: [u8; 32],
) -> Result<()> {
    let mut buf = vec![0u8; 65536 + 1024];

    loop {
        let (len, _peer) =
            match tokio::time::timeout(RECEIVE_TIMEOUT, socket.recv_from(&mut buf)).await {
                Ok(result) => result.context("recv_from failed")?,
                Err(_) => bail!(
                    "receive timeout — no data for {}s, session presumed dead",
                    RECEIVE_TIMEOUT.as_secs()
                ),
            };

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

        // Validate schema
        if let Some(schema) = KnownSchema::from_id(&header.schema_id) {
            if let Err(e) = schema.validate(&payload) {
                tracing::warn!(
                    schema = schema.name(),
                    error = %e,
                    content_hash = hex::encode(header.content_hash),
                    "chunk failed schema validation, discarding"
                );
                continue;
            }
            tracing::trace!(schema = schema.name(), "chunk validated");
        } else {
            tracing::trace!(
                schema_id = hex::encode(header.schema_id),
                "unknown schema, skipping validation"
            );
        }

        // Record delivery BEFORE caching (to track all arrivals)
        tracker.record(header.content_hash, peer_addr.clone());
        let delivery_count = tracker.delivery_count(&header.content_hash);

        // Cache the chunk
        if let Err(e) = cache.put(&header.content_hash, &payload) {
            tracing::warn!(error = %e, "failed to cache chunk");
        }

        let incoming = IncomingChunk {
            content_hash: header.content_hash,
            type_tag: header.type_tag,
            schema_id: header.schema_id,
            payload,
        };

        tracing::info!(
            content_hash = hex::encode(incoming.content_hash),
            type_tag = incoming.type_tag,
            payload_len = incoming.payload.len(),
            cached = true,
            delivery_count,
            peer = %peer_addr,
            "chunk received"
        );

        // Only deliver to application on FIRST receipt
        if delivery_count == 1 {
            // Try service dispatch first
            let dispatched = dispatcher.dispatch(&peer_pubkey, &header, &incoming.payload);

            if !dispatched {
                // No service handled it — send on the general channel
                // for backward compatibility with existing handlers.
                if chunk_tx.send(incoming).await.is_err() {
                    bail!("chunk receiver dropped, terminating receive loop");
                }
            }
        } else {
            tracing::debug!(
                content_hash = hex::encode(incoming.content_hash),
                delivery_count,
                "duplicate chunk via multipath, deduplicating"
            );
        }
    }
}
