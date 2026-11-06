# PROGRESS

## Current state (2026-11-06)

Branch: main (uncommitted). Gate: `cargo fmt --check && cargo clippy --workspace --all-targets && cargo test --workspace --exclude integration && scripts/integration-rootless.sh`

### Done, verified locally
- Security: peer identity bound to the Noise-proven static key
  (`Session.remote_static`). Previously an unauthenticated multicast
  announcement decided who a session belonged to.
- `libsummit` holds the whole node (`Node::start` / `run` / `ShutdownHandle`);
  `summitd` is a thin wrapper. Every task is reaped on stop.
- Startup waits up to 15 s for a link-local address (DAD after link up)
  instead of exiting. Found by the integration suite; matters at boot and on
  Wi-Fi reconnect.
- Portability: interface lookup on Unix/Windows (or numeric index), compute
  rlimits Unix-only, `cmd /C` on Windows, HOME/USERPROFILE/temp fallback.
  `cargo check` passes for windows-gnu and macOS; arm64 Android binaries build
  with `scripts/build-android.sh`.
- `/api/status` reports the node's own `public_key`.
- summitd stops emitting ANSI colour codes when stdout isn't a terminal.
- summit-ctl: no TLS stack, no system proxy (localhost-only client).
- Integration suite runs without sudo: `scripts/integration-rootless.sh`.
  Harness fixes: per-spawn identity dirs (the suite had been broken since
  keypair persistence; both daemons shared one key), poison-tolerant lock,
  pkill scoped to the test binary, received-file path.
- `scripts/android-smoke.sh`: staged real-device test with diagnosis.
  Dry-run against a fake phone passes (`scripts/android-smoke-dryrun.sh`),
  including the `--filter-multicast` failure path.

- One-way multicast works. A responder accepts a HandshakeInit from a peer it
  never heard announce: identity is the Noise-proven key, and the peer is
  recorded in the registry as `discovered_via: "handshake"` (`/api/peers`).
  If the designated (lower-key) side hasn't connected after 7 s, the
  higher-key side initiates. Simultaneous Inits are resolved in favour of the
  designated initiator. Registry entries with a live session no longer expire.
  Covered by `one_way::*` integration tests (both key orderings, with a check
  that the multicast drop rule is real) and by
  `android-smoke-dryrun.sh --filter-multicast` (now all session/message/file
  stages pass; multicast stage reported as WARN).
- The initiator no longer holds registry (DashMap) guards across awaits, and
  registers a handshake before sending it (so a fast response can't race it).

- Vision/charter in README: "devices that share a link talk directly"; what
  "direct" means per link type; what Summit is not. Verified the daemon
  needs no root (session formed with all capabilities dropped), README `sudo`
  removed.

### Not verified
- Anything on a real phone (needs the user's device: docs/ANDROID.md, rungs 1–3).
- macOS/Windows runtime networking. CI `portability` and `android` jobs have
  never run.

### Integration suite: 8 failures, all pre-existing (failed before these changes)
- Stale tests: `compute::test_compute_task_simple`,
  `failures::test_compute_worker_crash` (expect "Queued", but the executor now
  runs tasks within 1 s), `status::test_ctl_schema_list` (schema names changed).
- `recovery::test_nack_recovers_file_under_packet_loss`,
  `recovery::test_nack_concurrent_file_recovery`,
  `recovery::test_nack_loop_no_interference_clean_transfer`,
  `failures::test_receiver_killed_and_resend`: file transfers lost. Confirmed
  one root cause: if the FileMetadata chunk is dropped, the receiver never
  creates an assembly, so the NACK loop has nothing to recover. The file is
  silently lost. Wi-Fi will hit this.
- `failures::test_trust_then_check_buffered_messages`: not investigated.

### Next
1. Real phone, rungs 1–3 of docs/ANDROID.md, plus 2b (Wi-Fi Direct).
2. Metadata-loss recovery (sender-side retransmit of metadata until the
   receiver acknowledges it).
3. Android app shell (docs/ANDROID.md rung 4) + API auth token.
