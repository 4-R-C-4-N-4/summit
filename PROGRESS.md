# PROGRESS

## Current state (2026-11-06)

Branch: main. Gate (same as CI): `cargo fmt --all -- --check && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace --lib && scripts/integration-rootless.sh -- --test-threads=1 && scripts/android-smoke-dryrun.sh [--filter-multicast]`

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

- Trust gate on the receive path: untrusted peers' chunks are buffered
  before any service, cache or disk write sees them; blocked peers' are
  dropped; untrusted peers can't trigger retransmission (NACK/GONE).
  Received files are written 0644, never executable.
- Reconnect after restart: a new handshake from a known identity supersedes
  the stale session (~10 s to recover instead of ~60 s).
- File delivery is confirmed. The sender re-sends a file's metadata every
  3 s (up to 10 sends) until the receiver answers FILE_DONE; the receiver
  re-answers for files it already completed. Fixes silent loss when the
  metadata chunk is dropped. New assemblies pull blocks already in the chunk
  cache, and files with repeated or shared blocks now complete (previously
  any file with two identical 32 KiB blocks never finished).

### Test suite: 58/58 integration, 150 unit (2026-11-06)
- Faults now hit the traffic they claim to. netem shapes only a namespace's
  outbound traffic, so the old "loss on the receiver" recovery tests lost
  NACKs, not data, and passed without ever recovering anything; several also
  accepted "file never arrived" as a pass. Data loss is now injected with
  `drop_every_nth_chunk` (ip6tables nth match on first IPv6 fragments):
  deterministic, guaranteed holes, control traffic untouched. Every recovery
  test requires the file to arrive intact.
- Transfer tests use `distinct_bytes` (repeated bytes collapse to one block
  and hid the repeated-block bug). One clean-transfer test keeps repetitive
  data on purpose as that regression test.
- Metadata loss covered end to end: full blackout during send, and exactly
  the metadata dropped while data lands. Negative control: with the
  metadata re-send disabled both fail; with it both pass.
- "Mid-transfer" tests (sender killed, sender blocked) force holes first; on
  a clean veth the whole file landed before the kill, so they tested nothing.
- With `SUMMIT_REQUIRE_INTEGRATION=1` (set by integration-rootless.sh) a
  missing environment fails the suite instead of skipping every test green.
- CI: new `integration` job (rootless namespaces on ubuntu-latest, preloaded
  netfilter/netem modules, Android dry runs); clippy with `-D warnings` and
  all targets. Not yet run on GitHub.

### Not verified
- Anything on a real phone (needs the user's device: docs/ANDROID.md, rungs 1–3).
- macOS/Windows runtime networking. CI `portability`, `android` and
  `integration` jobs have never run on GitHub (local cross `cargo check
  --all-targets` passes for windows-gnu and macOS).

### Risks to watch on real hardware
- Chunks are up to 32 KiB, sent as single UDP datagrams, so ~23 IPv6
  fragments each; losing any fragment loses the chunk. Wi-Fi frame retries
  usually hide this, but if real-link transfers crawl, shrink chunks to fit
  the MTU (the smoke script's diagnosis already flags "small works, bulk
  doesn't").
- Exhausted assemblies (sender gone) stay listed in progress until the 300 s
  stale timeout.

### Next
1. Real phone, rungs 1–3 of docs/ANDROID.md, plus 2b (Wi-Fi Direct).
2. Android app shell (docs/ANDROID.md rung 4) + API auth token.
