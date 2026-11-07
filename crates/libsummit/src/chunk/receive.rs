use std::sync::Arc;
use std::time::Duration;

use anyhow::{bail, Context, Result};
use bytes::Bytes;
use tokio::net::UdpSocket;
use tokio::sync::{mpsc, Mutex};
use zerocopy::FromBytes;

use summit_core::crypto::{hash, Session};
use summit_core::recovery::{Capacity, FileDone, Gone, Nack};
use summit_core::wire::{self, ChunkHeader, MAX_UDP_BUF};
use summit_services::{
    ChunkCache, FileReassembler, KnownSchema, OutgoingChunk, SendTarget, TokenBucket, TrustLevel,
    TrustRegistry, UntrustedBuffer,
};

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
    bucket: Arc<Mutex<TokenBucket>>,
    reassembler: Arc<FileReassembler>,
    trust: TrustRegistry,
    untrusted: UntrustedBuffer,
) -> Result<()> {
    let mut buf = vec![0u8; MAX_UDP_BUF];

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

        // Trust gate. Must run before anything acts on the chunk: services
        // write files, store messages and queue compute tasks, and the cache
        // writes to disk.
        match admit(trust.check(&peer_pubkey), &header) {
            Admit::Process => {}
            Admit::Drop => {
                tracing::debug!(
                    peer = hex::encode(&peer_pubkey[..8]),
                    type_tag = { header.type_tag },
                    "chunk from blocked/untrusted peer dropped"
                );
                continue;
            }
            Admit::Buffer => {
                tracing::info!(
                    peer = hex::encode(&peer_pubkey[..8]),
                    content_hash = hex::encode(&header.content_hash[..8]),
                    "chunk from untrusted peer, buffering until trusted"
                );
                untrusted.add(
                    peer_pubkey,
                    header.content_hash,
                    header.type_tag,
                    header.schema_id,
                    payload,
                );
                continue;
            }
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

        // Only deliver to application on FIRST receipt. Exceptions, both
        // idempotent: file metadata (senders re-send it until confirmed and
        // the reassembler must answer FILE_DONE again), and FILE_DONE itself
        // (identical bytes from different recipients of the same file).
        let always_deliver = header.schema_id == KnownSchema::FileMetadata.id()
            || (header.schema_id == wire::recovery_hash() && { header.type_tag }
                == wire::recovery::FILE_DONE);
        if delivery_count == 1 || always_deliver {
            // Handle recovery protocol directly (below service layer)
            if header.schema_id == wire::recovery_hash() {
                handle_recovery(
                    &header,
                    &incoming.payload,
                    &peer_pubkey,
                    &cache,
                    &outbound_tx,
                    &bucket,
                    &reassembler,
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
    bucket: &Arc<Mutex<TokenBucket>>,
    reassembler: &Arc<FileReassembler>,
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

            // Pace retransmissions based on the receiver's advertised capacity.
            // Realtime priority bypasses the token bucket, so pacing is the
            // only rate control for recovery traffic.
            let (batch_size, batch_delay_ms) = {
                let b = bucket.lock().await;
                let burst = (b.capacity() as u32).max(8);
                let rate = (b.rate() as u32).max(8);
                let delay = ((burst as u64) * 1000 / (rate as u64)).max(50);
                (burst, delay)
            };

            let mut retransmitted = 0u32;
            for content_hash in &nack.missing {
                match cache.get(content_hash) {
                    Ok(Some(data)) => {
                        let chunk = OutgoingChunk {
                            type_tag: 2, // file data
                            schema_id: KnownSchema::FileData.id(),
                            payload: data,
                            priority_flags: 0x01, // Realtime — bypass token bucket for recovery
                        };
                        let target = SendTarget::Peer {
                            public_key: *peer_pubkey,
                        };
                        if let Err(e) = chunk_tx.send((target, chunk)).await {
                            tracing::warn!(error = %e, "failed to enqueue retransmit");
                            return;
                        }
                        retransmitted += 1;
                        if retransmitted.is_multiple_of(batch_size) {
                            tokio::time::sleep(std::time::Duration::from_millis(batch_delay_ms))
                                .await;
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
                        priority_flags: 0x01, // Realtime — recovery protocol must not be rate-limited
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
            let gone: Gone = match serde_json::from_slice(payload) {
                Ok(g) => g,
                Err(e) => {
                    tracing::warn!(error = %e, "invalid GONE payload");
                    return;
                }
            };
            // Don't abandon immediately — broadcast NACKs (attempt 1+) may
            // recover chunks from other peers. Log for diagnostics and let
            // the NACK stall counter handle exhaustion naturally.
            let stalled = reassembler.missing_chunks().await;
            for (filename, missing) in &stalled {
                let gone_count = missing.iter().filter(|h| gone.hashes.contains(h)).count();
                if gone_count > 0 {
                    tracing::warn!(
                        peer = hex::encode(&peer_pubkey[..8]),
                        filename,
                        gone = gone_count,
                        still_missing = missing.len(),
                        "peer reports chunks GONE — will retry via broadcast"
                    );
                }
            }
        }

        wire::recovery::FILE_DONE => match serde_json::from_slice::<FileDone>(payload) {
            Ok(done) => {
                reassembler
                    .confirm_outgoing(&done.metadata_hash, peer_pubkey)
                    .await
            }
            Err(e) => tracing::warn!(error = %e, "invalid FILE_DONE payload"),
        },

        wire::recovery::CAPACITY => {
            let cap: Capacity = match serde_json::from_slice(payload) {
                Ok(c) => c,
                Err(e) => {
                    tracing::warn!(error = %e, "invalid CAPACITY payload");
                    return;
                }
            };
            let rate = if cap.bulk_rate == 0 {
                128.0
            } else {
                cap.bulk_rate as f64
            };
            let burst = if cap.bulk_burst == 0 {
                64.0
            } else {
                cap.bulk_burst as f64
            };
            bucket.lock().await.reconfigure(rate, burst);
            tracing::info!(
                rate,
                burst,
                "peer advertised bulk capacity, token bucket reconfigured"
            );
        }

        _ => {
            tracing::warn!(type_tag, "unknown recovery type_tag");
        }
    }
}

/// What to do with a chunk, given the sender's trust level.
#[derive(Debug, PartialEq, Eq)]
enum Admit {
    Process,
    /// Hold until the user trusts the peer, then replay through the services.
    Buffer,
    Drop,
}

fn admit(level: TrustLevel, header: &ChunkHeader) -> Admit {
    let recovery = header.schema_id == wire::recovery_hash();
    let type_tag = header.type_tag; // packed struct: copy out
    match level {
        TrustLevel::Trusted => Admit::Process,
        TrustLevel::Blocked => Admit::Drop,
        // Capacity is flow control for chunks we may send them; harmless.
        TrustLevel::Untrusted if recovery && type_tag == wire::recovery::CAPACITY => Admit::Process,
        // NACK/GONE would make us retransmit cached content to them.
        TrustLevel::Untrusted if recovery => Admit::Drop,
        TrustLevel::Untrusted => Admit::Buffer,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn header(schema_id: [u8; 32], type_tag: u16) -> ChunkHeader {
        ChunkHeader {
            content_hash: [0; 32],
            schema_id,
            type_tag,
            length: 0,
            flags: 0,
            version: 1,
        }
    }

    #[test]
    fn untrusted_service_chunks_are_buffered_not_processed() {
        let msg = header(summit_services::messaging_schema_id(), 0);
        let file = header(KnownSchema::FileData.id(), 2);
        assert_eq!(admit(TrustLevel::Untrusted, &msg), Admit::Buffer);
        assert_eq!(admit(TrustLevel::Untrusted, &file), Admit::Buffer);
        assert_eq!(admit(TrustLevel::Trusted, &msg), Admit::Process);
    }

    #[test]
    fn blocked_peers_get_nothing() {
        let cap = header(wire::recovery_hash(), wire::recovery::CAPACITY);
        let msg = header(summit_services::messaging_schema_id(), 0);
        assert_eq!(admit(TrustLevel::Blocked, &cap), Admit::Drop);
        assert_eq!(admit(TrustLevel::Blocked, &msg), Admit::Drop);
    }

    #[test]
    fn untrusted_may_send_capacity_but_not_nack() {
        let cap = header(wire::recovery_hash(), wire::recovery::CAPACITY);
        let nack = header(wire::recovery_hash(), wire::recovery::NACK);
        assert_eq!(admit(TrustLevel::Untrusted, &cap), Admit::Process);
        assert_eq!(admit(TrustLevel::Untrusted, &nack), Admit::Drop);
        assert_eq!(admit(TrustLevel::Trusted, &nack), Admit::Process);
    }
}
