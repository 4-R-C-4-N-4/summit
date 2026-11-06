# Summit on Android (and other non-Linux hosts)

## Where things stand

- The node is a library now: `crates/libsummit` (`Node::start` → `Node::run`,
  `ShutdownHandle` to stop it from any thread). `summitd` is a ~40-line wrapper.
  Stopping a node aborts and reaps every task, so its sockets are freed and it
  can be restarted inside the same process (needed for an Android service).
- OS-specific code is gated: interface lookup (`libsummit/src/net.rs`, Unix and
  Windows, or pass a numeric index), compute rlimits (Unix only; warns
  elsewhere), compute shell (`sh -c`, or `cmd /C` on Windows).
- `scripts/build-android.sh` cross-compiles `summitd` + `summit-ctl` for arm64
  Android with just the NDK. They run straight from `adb shell`, no app needed.
- macOS and Windows build in CI (`portability` job). Not tested on real
  networks yet.
- No Android app yet. That is step 4 below, and is only worth building once
  steps 1–3 pass on your phone.

## Testing connectivity without a lab of hardware

The trap: anything that runs entirely on one machine (netns, Docker, the
Android emulator) passes for reasons that don't hold on a real phone. The
emulator in particular puts the guest behind user-mode NAT with no link-local
multicast, so it can't even test discovery. Don't use it for networking.

So the plan is a ladder. Each rung adds one variable, and every rung except
0 needs only this computer and the phone you already carry.

### Rung 0: sandbox, every change (no hardware)

    scripts/integration-rootless.sh         # full integration suite, no sudo
    scripts/android-smoke-dryrun.sh         # smoke script vs. a fake phone

The integration suite runs two real daemons over a veth pair in throwaway
namespaces. The dry-run executes `android-smoke.sh` end to end against a
namespace pretending to be the phone. That proves the *script* works, so a
failure with the real phone is always the phone or the network.

### Rung 1: phone over USB tethering (deterministic wire)

    scripts/build-android.sh && cargo build
    # phone: Settings → Network → Hotspot & tethering → USB tethering
    scripts/android-smoke.sh --host-iface <usb0|enp…u…> --phone-iface <rndis0|ncm0>

A point-to-point Ethernet link: no AP, no Wi-Fi power save, no client
isolation. If this fails, the bug is in our code on Android (bionic, the
kernel's IPv6 config), not the network. Find the phone's interface with
`adb shell ip -6 addr`.

### Rung 2: through a Wi-Fi access point

    scripts/android-smoke.sh --host-iface wlp5s0 --phone-iface wlan0

The script walks the stages in order and prints a diagnosis. The asymmetric
case is the one to expect:

| phone hears host | host hears phone | session | meaning |
|---|---|---|---|
| WARN | PASS | PASS | Expected from `adb shell`: Android filters inbound multicast without a `MulticastLock`. The phone learned the host from the handshake (one-way fallback), so it works but the phone can only find peers that can hear it. The app (rung 4) fixes this. |
| WARN | WARN | FAIL | No multicast either way: the AP drops multicast or isolates clients (guest/public/mesh networks), or wrong interface. |
| any | any | FAIL | Unicast blocked (host firewall on UDP?), or a bug in the fallback: grep logs for `initiating as fallback` / `learned from handshake`. |

Retry with the phone's hotspot (host joins it). The phone is then the AP,
which takes client isolation out of the picture.

### Rung 2b: Wi-Fi Direct (no access point at all)

The purest form of "direct": two radios, nothing else. Android and Linux
expose a Wi-Fi Direct group as an ordinary interface (`p2p-wlan0-0`,
`p2p-wlp5s0-0`, …) with an IPv6 link-local address, which is all Summit needs.

    # laptop: wpa_cli p2p_find / p2p_connect <phone-mac> pbc
    # phone:  Settings → Wi-Fi → Wi-Fi Direct, accept the laptop
    adb shell ip -6 addr          # find the phone's p2p-* interface
    scripts/android-smoke.sh --host-iface p2p-wlp5s0-0 --phone-iface p2p-wlan0-0

Untested. Open questions: whether a non-app (`adb shell`) process can use the
group interface, and whether the group survives without an app holding it.

### Rung 3: screen off

    scripts/android-smoke.sh … --screen-off 120

Doze and Wi-Fi power save are where "works on my desk" apps die. From
`adb shell` this is expected to degrade. The number to care about is how the
app (rung 4) does.

### Rung 4: the app

Smallest useful shell (Kotlin, ~300 lines):

- Foreground service (`connectedDevice` type) that holds a
  `WifiManager.MulticastLock` and a partial wake lock while running.
- JNI shim over `libsummit`: `start(configJson, dataDir) -> handle`,
  `stop(handle)`. The service gives it `context.filesDir`, since Android apps
  have no `$HOME`.
- WebView showing the existing Zenith React build, talking to the node's
  local API. Zenith only talks HTTP to the API, so the UI comes nearly free.
- Compute disabled. An API auth token is required before shipping, because
  any app on the phone can reach 127.0.0.1.

`android-smoke.sh` keeps working against the app: point it at the app's API
port via `adb forward` instead of launching the binary. That gives the same
staged diagnosis for the shipped configuration.

## Known gaps

- Announcements are unsigned. A forged announcement can't impersonate a peer
  (identity comes from the handshake), and a completed handshake now
  overrides the announced address. A forger can still advertise a wrong
  address for a peer we haven't connected to yet, which delays that peer
  until the fallback initiation runs the other way.
- The local API has no authentication. Fine on a single-user Linux box; not
  on a phone.
- iOS needs Apple's multicast entitlement, and its background limits are
  stricter. Out of scope until Android works.
