//! Recovery loop — periodically checks for stalled file assemblies
//! and sends NACKs to request retransmission of missing chunks.

use std::sync::Arc;
use std::time::Duration;

use summit_core::recovery::{Nack, MAX_NACK_HASHES};
use summit_core::wire;
use summit_services::{FileReassembler, OutgoingChunk, SendTarget};
use tokio::sync::{broadcast, mpsc};

/// How long to wait after the last chunk before sending a NACK.
const NACK_DELAY: Duration = Duration::from_secs(3);

/// How often to check for stalled assemblies.
const CHECK_INTERVAL: Duration = Duration::from_secs(2);

pub async fn recovery_loop(
    reassembler: Arc<FileReassembler>,
    chunk_tx: mpsc::Sender<(SendTarget, OutgoingChunk)>,
    mut shutdown: broadcast::Receiver<()>,
) {
    let mut interval = tokio::time::interval(CHECK_INTERVAL);

    loop {
        tokio::select! {
            _ = shutdown.recv() => {
                tracing::info!("recovery loop shutting down");
                return;
            }
            _ = interval.tick() => {
                send_nacks(&reassembler, &chunk_tx).await;
            }
        }
    }
}

async fn send_nacks(
    reassembler: &FileReassembler,
    chunk_tx: &mpsc::Sender<(SendTarget, OutgoingChunk)>,
) {
    let stalled = reassembler.stalled_assemblies(NACK_DELAY).await;

    for assembly in stalled {
        if assembly.missing.is_empty() {
            continue;
        }

        // Attempt 0: target the original sender directly.
        // Attempt 1+: broadcast to all peers — anyone with the chunks can help.
        let target = if assembly.attempt == 0 {
            SendTarget::Peer {
                public_key: assembly.sender_pubkey,
            }
        } else {
            SendTarget::Broadcast
        };

        // Split into batches of MAX_NACK_HASHES
        for batch in assembly.missing.chunks(MAX_NACK_HASHES) {
            let nack = Nack {
                missing: batch.to_vec(),
                attempt: assembly.attempt,
            };

            let payload = match serde_json::to_vec(&nack) {
                Ok(p) => p,
                Err(e) => {
                    tracing::warn!(error = %e, "failed to serialize NACK");
                    continue;
                }
            };

            let chunk = OutgoingChunk {
                type_tag: wire::recovery::NACK,
                schema_id: wire::recovery_hash(),
                payload: bytes::Bytes::from(payload),
                priority_flags: 0x02,
            };

            if let Err(e) = chunk_tx.send((target.clone(), chunk)).await {
                tracing::warn!(error = %e, "failed to send NACK");
            }
        }

        reassembler.increment_nack_count(&assembly.filename).await;

        tracing::info!(
            filename = assembly.filename,
            missing = assembly.missing.len(),
            attempt = assembly.attempt,
            targeted = assembly.attempt == 0,
            "NACK sent for stalled file assembly"
        );
    }
}
