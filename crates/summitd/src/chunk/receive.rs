use std::sync::Arc;
use std::time::Duration;

use anyhow::{bail, Context, Result};
use bytes::Bytes;
use tokio::net::UdpSocket;
use tokio::sync::{mpsc, Mutex};
use zerocopy::FromBytes;

use summit_core::crypto::{hash, Session};
use summit_core::recovery::{Gone, Nack};
use summit_core::wire::{self, ChunkHeader};
use summit_services::{ChunkCache, KnownSchema, OutgoingChunk, SendTarget};

/// How long to wait for data before considering the session dead.
const RECEIVE_TIMEOUT: Duration = Duration::from_secs(60);

use super::IncomingChunk;

use crate::delivery::DeliveryTracker;
use crate::dispatch::ServiceDispatcher;

#[allow(clippy::too_many_arguments)]
pub async fn receive_loop(
    socket: Arc<UdpSocket>,
    session: Arc<Mutex<Session>>,
    chunk_tx: mpsc::Sender<IncomingChunk>,
    outbound_tx: mpsc::Sender<(SendTarget, OutgoingChunk)>,
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
            // Handle recovery protocol directly (below service layer)
            if header.schema_id == wire::recovery_hash() {
                handle_recovery(
                    &header,
                    &incoming.payload,
                    &peer_pubkey,
                    &cache,
                    &outbound_tx,
                )
                .await;
                continue;
            }

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

async fn handle_recovery(
    header: &ChunkHeader,
    payload: &[u8],
    peer_pubkey: &[u8; 32],
    cache: &ChunkCache,
    chunk_tx: &mpsc::Sender<(SendTarget, OutgoingChunk)>,
) {
    let type_tag = header.type_tag;
    match type_tag {
        wire::recovery::NACK => {
            let nack: Nack = match serde_json::from_slice(payload) {
                Ok(n) => n,
                Err(e) => {
                    tracing::warn!(error = %e, "invalid NACK payload");
                    return;
                }
            };

            let is_targeted = nack.attempt == 0;

            tracing::info!(
                peer = hex::encode(&peer_pubkey[..8]),
                missing = nack.missing.len(),
                attempt = nack.attempt,
                targeted = is_targeted,
                "NACK received, retransmitting cached chunks"
            );

            let mut gone_hashes = Vec::new();

            for content_hash in &nack.missing {
                match cache.get(content_hash) {
                    Ok(Some(data)) => {
                        let chunk = OutgoingChunk {
                            type_tag: 2, // file data
                            schema_id: KnownSchema::FileData.id(),
                            payload: data,
                            priority_flags: 0x02,
                        };
                        let target = SendTarget::Peer {
                            public_key: *peer_pubkey,
                        };
                        if let Err(e) = chunk_tx.send((target, chunk)).await {
                            tracing::warn!(error = %e, "failed to enqueue retransmit");
                            return;
                        }
                    }
                    Ok(None) => {
                        gone_hashes.push(*content_hash);
                    }
                    Err(e) => {
                        tracing::warn!(
                            error = %e,
                            hash = hex::encode(content_hash),
                            "cache read error during retransmit"
                        );
                        gone_hashes.push(*content_hash);
                    }
                }
            }

            // Only send GONE for targeted NACKs (attempt 0).
            // On broadcast NACKs, peers that don't have the chunk just stay silent.
            if is_targeted && !gone_hashes.is_empty() {
                let gone = Gone {
                    hashes: gone_hashes,
                };
                if let Ok(payload) = serde_json::to_vec(&gone) {
                    let chunk = OutgoingChunk {
                        type_tag: wire::recovery::GONE,
                        schema_id: wire::recovery_hash(),
                        payload: bytes::Bytes::from(payload),
                        priority_flags: 0x02,
                    };
                    let _ = chunk_tx
                        .send((
                            SendTarget::Peer {
                                public_key: *peer_pubkey,
                            },
                            chunk,
                        ))
                        .await;
                }
            }
        }

        wire::recovery::GONE => {
            // GONE is handled by the chunk manager (receiver side).
            // It arrives here via the general chunk channel.
            tracing::debug!(
                peer = hex::encode(&peer_pubkey[..8]),
                "GONE received — forwarded via chunk channel"
            );
        }

        _ => {
            tracing::warn!(type_tag, "unknown recovery type_tag");
        }
    }
}
