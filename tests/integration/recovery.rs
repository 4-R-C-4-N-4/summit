use crate::fault::*;
use crate::*;
use std::process::Child;

// ══════════════════════════════════════════════════════════════════════════════
//  File recovery — NACK retransmission and metadata re-send
// ══════════════════════════════════════════════════════════════════════════════
//
// Faults are injected where the data actually flows. Data goes A -> B, so
// chunk loss is applied to B's INBOUND path with `drop_every_nth_chunk`
// (deterministic: holes are guaranteed, so recovery is always exercised).
// netem (`add_packet_loss`) only shapes a namespace's OUTBOUND traffic; on B
// it loses NACKs and FILE_DONE, not data.
//
// Every test asserts the file arrives intact. "Didn't arrive, but nothing
// crashed" is not a pass: on a real link that is the bug.

const RECEIVED: &str = "/tmp/summit-received";

/// Run `body` against an auto-trusting A/B pair with an established session.
/// `body` gets A's process so it can kill the sender.
fn with_pair(files: &[&str], body: impl FnOnce(&mut Child) -> Result<()>) {
    if !skip_unless_ready() {
        return;
    }
    let _lock = daemon_lock();
    cleanup_summitd();
    std::fs::remove_dir_all(RECEIVED).ok();

    let auto_env = [("SUMMIT_TRUST__AUTO_TRUST", "true")];
    let mut node_a = spawn_daemon(NS_A, VETH_A, &auto_env);
    let mut node_b = spawn_daemon(NS_B, VETH_B, &auto_env);

    let result = (|| -> Result<()> {
        wait_for_api(NS_A, 40)?;
        wait_for_api(NS_B, 40)?;
        wait_for_session(8)?;
        body(&mut node_a)
    })();

    node_a.kill().ok();
    node_b.kill().ok();
    cleanup_summitd();
    for f in files {
        std::fs::remove_file(f).ok();
    }
    result.unwrap();
}

fn send(path: &str) -> Result<()> {
    let out = ctl(NS_A, &["send", path])?;
    anyhow::ensure!(out.contains("File queued"), "send failed: {out}");
    Ok(())
}

fn received_path(sent: &str) -> String {
    let name = std::path::Path::new(sent).file_name().unwrap();
    format!("{RECEIVED}/{}", name.to_str().unwrap())
}

/// Every 3rd data chunk is lost on the way in. NACK recovery must request
/// and receive the holes. (Retransmissions are subject to the same loss.)
#[test]
fn test_nack_recovers_file_under_packet_loss() {
    let path = "/tmp/summit-test-nack-recovery.bin";
    let data = distinct_bytes(256 * 1024, 1); // 8 chunks
    std::fs::write(path, &data).unwrap();

    with_pair(&[path], |_| {
        let _loss = drop_every_nth_chunk(NS_B, 3);
        send(path)?;
        wait_for_file(&received_path(path), &data, 30)?;
        let dropped = dropped_chunks(NS_B);
        assert!(dropped >= 3, "fault not applied (dropped {dropped})");
        println!("recovered intact; {dropped} chunks dropped on the way");
        Ok(())
    });
}

/// Receiver is completely deaf while the file is sent: metadata and all data
/// are lost, so the receiver doesn't know the file exists and can't NACK.
/// Only the sender re-sending metadata (until FILE_DONE) can recover this.
#[test]
fn test_nack_recovers_after_temporary_blackout() {
    let path = "/tmp/summit-test-nack-blackout.bin";
    let data = distinct_bytes(96 * 1024, 8);
    std::fs::write(path, &data).unwrap();

    with_pair(&[path], |_| {
        let blackout = block_all_udp(NS_B);
        send(path)?;
        thread::sleep(Duration::from_secs(5));
        assert_eq!(files_in_progress(NS_B), 0, "B saw the file during blackout");
        assert!(
            !std::path::Path::new(&received_path(path)).exists(),
            "blackout did not block the transfer"
        );
        drop(blackout);
        println!("network restored; sender must re-send metadata");
        wait_for_file(&received_path(path), &data, 30)
    });
}

/// Only the metadata chunk is lost; every data chunk arrives (and is
/// cached). The re-sent metadata must complete the file from the cache.
#[test]
fn test_lost_metadata_completes_from_cached_data() {
    let path = "/tmp/summit-test-lost-metadata.bin";
    // 16 chunks: the metadata lists 16 hashes, large enough to fragment,
    // and it is the first large datagram sent.
    let data = distinct_bytes(512 * 1024, 9);
    std::fs::write(path, &data).unwrap();

    with_pair(&[path], |_| {
        let _drop_first = drop_every_nth_chunk(NS_B, 1_000_000);
        send(path)?;
        // Data has landed, but without metadata B doesn't know the file
        // exists: nothing in progress (a lost data chunk would show 1).
        thread::sleep(Duration::from_millis(1500));
        assert_eq!(dropped_chunks(NS_B), 1, "fault not applied");
        assert_eq!(
            files_in_progress(NS_B),
            0,
            "dropped datagram was not the metadata"
        );
        wait_for_file(&received_path(path), &data, 20)?;
        Ok(())
    });
}

/// While a lossy transfer is stalled it is visible as in-progress; once
/// recovered it completes and leaves nothing in progress.
#[test]
fn test_nack_in_progress_visible_during_stall() {
    let path = "/tmp/summit-test-nack-progress.bin";
    let data = distinct_bytes(256 * 1024, 2);
    std::fs::write(path, &data).unwrap();

    with_pair(&[path], |_| {
        let _loss = drop_every_nth_chunk(NS_B, 2);
        send(path)?;
        // Before the first NACK round (NACK_DELAY 2s) half the chunks are holes.
        thread::sleep(Duration::from_millis(1000));
        assert_eq!(files_in_progress(NS_B), 1, "stalled transfer not visible");
        wait_for_file(&received_path(path), &data, 40)?;
        wait_for_condition(5, || files_in_progress(NS_B) == 0)
            .context("completed file still listed in progress")
    });
}

/// 2 MiB (64 chunks) with every 4th lost — exercises NACK batching.
#[test]
fn test_nack_large_file_recovery() {
    let path = "/tmp/summit-test-nack-large.bin";
    let data = distinct_bytes(2 * 1024 * 1024, 10);
    std::fs::write(path, &data).unwrap();

    with_pair(&[path], |_| {
        let _loss = drop_every_nth_chunk(NS_B, 4);
        send(path)?;
        wait_for_file(&received_path(path), &data, 45)
    });
}

/// Sender dies with holes outstanding. Recovery is impossible: the receiver
/// must survive and must never commit a partial file.
#[test]
fn test_nack_sender_gone_assembly_abandoned() {
    let path = "/tmp/summit-test-nack-abandoned.bin";
    let data = distinct_bytes(512 * 1024, 7);
    std::fs::write(path, &data).unwrap();

    with_pair(&[path], |node_a| {
        let _loss = drop_every_nth_chunk(NS_B, 2); // guaranteed holes
        send(path)?;
        thread::sleep(Duration::from_millis(500));
        node_a.kill().ok();
        node_a.wait().ok();

        // NACK_DELAY + MAX_NACK_STALLS rounds against a dead sender.
        thread::sleep(Duration::from_secs(12));
        assert!(daemon_alive(NS_B), "receiver died during failed recovery");
        assert!(
            !std::path::Path::new(&received_path(path)).exists(),
            "partial file committed though the sender died with holes"
        );
        Ok(())
    });
}

/// Five files in flight at once, every 3rd chunk lost: all must recover.
#[test]
fn test_nack_concurrent_file_recovery() {
    let files: Vec<(String, Vec<u8>)> = (0..5)
        .map(|i| {
            let path = format!("/tmp/summit-test-nack-concurrent-{i}.bin");
            let data = distinct_bytes((i + 1) * 64 * 1024, 100 + i as u32);
            std::fs::write(&path, &data).unwrap();
            (path, data)
        })
        .collect();
    let paths: Vec<&str> = files.iter().map(|(p, _)| p.as_str()).collect();

    with_pair(&paths, |_| {
        let _loss = drop_every_nth_chunk(NS_B, 3);
        for (path, _) in &files {
            send(path)?;
        }
        for (path, data) in &files {
            wait_for_file(&received_path(path), data, 45)?;
        }
        Ok(())
    });
}

/// Loss in both directions: data chunks lost on the way in, and 20% of
/// everything B sends (NACKs, FILE_DONE) lost on the way out. Recovery must
/// retry lost NACKs.
#[test]
fn test_nack_escalation_under_sender_loss() {
    let path = "/tmp/summit-test-nack-escalation.bin";
    let data = distinct_bytes(256 * 1024, 3);
    std::fs::write(path, &data).unwrap();

    with_pair(&[path], |_| {
        let _data_loss = drop_every_nth_chunk(NS_B, 3);
        let _nack_loss = add_packet_loss(NS_B, VETH_B, 20);
        send(path)?;
        wait_for_file(&received_path(path), &data, 45)
    });
}

/// Clean network: the file arrives promptly and recovery leaves no residue.
/// Deliberately repetitive data — 8 identical 32 KiB blocks, one distinct
/// hash — as a regression test for files with repeated blocks completing.
#[test]
fn test_nack_loop_no_interference_clean_transfer() {
    let path = "/tmp/summit-test-nack-clean.bin";
    let data: Vec<u8> = (0..256 * 1024).map(|i| (i % 256) as u8).collect();
    std::fs::write(path, &data).unwrap();

    with_pair(&[path], |_| {
        send(path)?;
        wait_for_file(&received_path(path), &data, 8)?;
        assert_eq!(files_in_progress(NS_B), 0, "residual in-progress assembly");
        Ok(())
    });
}
