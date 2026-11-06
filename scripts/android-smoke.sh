#!/usr/bin/env bash
# Real-hardware smoke test: this machine <-> an Android phone over a real link.
#
# No app required. Pushes the cross-compiled summitd/summit-ctl to the phone
# over adb, runs a daemon on each side, and walks the protocol stage by stage
# so a failure says WHERE it broke, not just that it did:
#
#   1. phone hears host announcements      (inbound multicast on the phone)
#   2. host hears phone announcements      (outbound multicast from the phone)
#   3. Noise session established both ways (unicast UDP, link-local)
#      Stages 1-2 are WARN, not FAIL: one-way multicast still yields a session
#      (the deaf side learns its peer from the handshake). It does mean the
#      deaf side can only find peers that can hear it.
#   4. message host -> phone and back      (encrypted chunk path)
#   5. 1 MiB file host -> phone            (bulk transfer, NACK recovery)
#   6. optional: screen off N seconds, then repeat 3-4  (Doze / Wi-Fi filters)
#
# Links to try, in order of how many variables they add:
#   usb   USB tethering on the phone; host iface is usb0/enp*u*, phone iface is
#         rndis0 or ncm0. A point-to-point wire: proves the Android build and
#         protocol without any Wi-Fi behaviour in the way.
#   wifi  Both on the same Wi-Fi. Proves the real network (AP multicast
#         handling, client isolation, power-save filters).
#   hotspot  Phone's hotspot with the host joined. The phone is the AP, so
#         client isolation can't apply; useful when "wifi" fails.
#
# Usage:
#   scripts/build-android.sh
#   scripts/android-smoke.sh --host-iface wlp5s0 --phone-iface wlan0 [--screen-off 120]
#
# Requires: adb (device authorised), curl, python3. Uses auto-trust on both
# sides — test only.
set -uo pipefail
cd "$(dirname "$0")/.."

HOST_IFACE=""
PHONE_IFACE="wlan0"
SCREEN_OFF=0
TARGET="${ANDROID_TARGET:-aarch64-linux-android}"
HOST_API=19001
FWD_API=19101
STAGE_TIMEOUT=30

while [[ $# -gt 0 ]]; do
    case "$1" in
        --host-iface) HOST_IFACE="$2"; shift 2 ;;
        --phone-iface) PHONE_IFACE="$2"; shift 2 ;;
        --screen-off) SCREEN_OFF="$2"; shift 2 ;;
        --timeout) STAGE_TIMEOUT="$2"; shift 2 ;;
        -h|--help) sed -n '2,32p' "$0"; exit 0 ;;
        *) echo "unknown arg: $1" >&2; exit 2 ;;
    esac
done

ADB="${ADB:-$(command -v adb || echo "$HOME/Android/platform-tools/adb")}"
[[ -x "$ADB" ]] || { echo "adb not found (set ADB=)" >&2; exit 2; }
[[ -n "$HOST_IFACE" ]] || HOST_IFACE="$(ip route show default | awk '{print $5; exit}')"

# Overridable so the script can be dry-run against a fake adb (see
# scripts/android-smoke-dryrun.sh); defaults are the real-phone values.
PHONE_DIR="${PHONE_DIR:-/data/local/tmp/summit}"
PHONE_BIN_DIR="${PHONE_BIN_DIR:-${CARGO_TARGET_DIR:-target}/$TARGET/release}"
HOST_BIN_DIR="${HOST_BIN_DIR:-${CARGO_TARGET_DIR:-target}/debug}"
OUT="$(mktemp -d "${TMPDIR:-/tmp}/summit-smoke.XXXXXX")"
HOST_HOME="$OUT/host-home"
mkdir -p "$HOST_HOME"
HOST_PID=""
declare -A RESULT
# Bracket trick: the pattern must not match the `sh -c "pkill …"` that runs it,
# or pkill kills its own shell.
PHONE_DAEMON_PAT="$PHONE_DIR/[s]ummitd"

log() { printf '%s\n' "$*" | tee -a "$OUT/smoke.log"; }

cleanup() {
    [[ -n "$HOST_PID" ]] && kill "$HOST_PID" 2>/dev/null
    "$ADB" shell "pkill -f '$PHONE_DAEMON_PAT'" >/dev/null 2>&1
    "$ADB" shell input keyevent KEYCODE_WAKEUP >/dev/null 2>&1
    "$ADB" pull "$PHONE_DIR/summitd.log" "$OUT/phone-summitd.log" >/dev/null 2>&1
    "$ADB" forward --remove "tcp:$FWD_API" >/dev/null 2>&1
    log ""
    log "logs: $OUT"
}
trap cleanup EXIT

host_api() { curl -sf "http://127.0.0.1:$HOST_API/api$1" "${@:2}"; }
phone_api() { curl -sf "http://127.0.0.1:$FWD_API/api$1" "${@:2}"; }
# json <python-expr over d>: prints the value, or nothing if input isn't JSON
# (e.g. curl failed while the daemon is still starting).
json() {
    python3 -c "import json,sys
try: d=json.load(sys.stdin)
except ValueError: sys.exit(1)
print($1)" 2>/dev/null
}

# wait_until <seconds> <command...>: retry once a second.
wait_until() {
    local deadline=$((SECONDS + $1)); shift
    while ((SECONDS < deadline)); do
        "$@" && return 0
        sleep 1
    done
    return 1
}

stage() { # stage <id> <description> <command...>
    local id="$1" desc="$2"; shift 2
    if wait_until "$STAGE_TIMEOUT" "$@"; then
        RESULT[$id]=PASS; log "  PASS  $desc"
    else
        RESULT[$id]=${MISS:-FAIL}; log "  ${MISS:-FAIL}  $desc"
    fi
}

# ── Setup ─────────────────────────────────────────────────────────────────────
log "host iface: $HOST_IFACE   phone iface: $PHONE_IFACE   out: $OUT"
"$ADB" get-state >/dev/null || { log "no adb device"; exit 2; }
log "phone: $("$ADB" shell getprop ro.product.model) / Android $("$ADB" shell getprop ro.build.version.release)"

bin="$PHONE_BIN_DIR"
[[ -x "$bin/summitd" ]] || { log "missing $bin/summitd — run scripts/build-android.sh"; exit 2; }
[[ -x "$HOST_BIN_DIR/summitd" ]] || { log "missing $HOST_BIN_DIR/summitd — run cargo build"; exit 2; }
# Preflight: the phone's toybox must have what the stages use. Missing tools
# should fail here with their names, not later as a fake "network" failure.
missing="$("$ADB" shell 'for t in ip pkill nohup sha256sum; do command -v $t >/dev/null 2>&1 || echo $t; done' | tr -d '\r' | xargs)"
[[ -z "$missing" ]] || { log "phone is missing shell tools: $missing"; exit 2; }
# setsid is optional (absent from early toybox builds); nohup alone is enough
# for the daemon to survive the adb session in practice.
SETSID="$("$ADB" shell 'command -v setsid >/dev/null 2>&1 && echo setsid' | tr -d '\r')"

"$ADB" shell "mkdir -p $PHONE_DIR; pkill -f '$PHONE_DAEMON_PAT'" >/dev/null 2>&1
"$ADB" push "$bin/summitd" "$bin/summit-ctl" "$PHONE_DIR/" >/dev/null || exit 2
"$ADB" shell "chmod 755 $PHONE_DIR/summitd $PHONE_DIR/summit-ctl"

log "phone link-local:"
"$ADB" shell "ip -6 addr show dev $PHONE_IFACE scope link" | tee -a "$OUT/smoke.log"

# Phone daemon. (setsid+)nohup so it outlives the adb shell session.
"$ADB" shell "cd $PHONE_DIR && rm -rf home && mkdir -p home/received && \
    HOME=$PHONE_DIR/home RUST_LOG=info SUMMIT_TRUST__AUTO_TRUST=true \
    SUMMIT_FILE_TRANSFER__STORAGE_PATH=$PHONE_DIR/home/received \
    $SETSID nohup ./summitd $PHONE_IFACE > summitd.log 2>&1 < /dev/null &"
"$ADB" forward "tcp:$FWD_API" tcp:9001 >/dev/null

# Host daemon, isolated from any real install on this machine.
env -u XDG_CONFIG_HOME -u XDG_DATA_HOME HOME="$HOST_HOME" RUST_LOG=info \
    SUMMIT_TRUST__AUTO_TRUST=true SUMMIT_NETWORK__API_PORT=$HOST_API \
    SUMMIT_FILE_TRANSFER__STORAGE_PATH="$HOST_HOME/received" \
    "$HOST_BIN_DIR/summitd" "$HOST_IFACE" >"$OUT/host-summitd.log" 2>&1 &
HOST_PID=$!

wait_until 15 host_api /status >/dev/null || { log "host API never came up"; exit 1; }
wait_until 15 phone_api /status >/dev/null || { log "phone API never came up (see phone log)"; exit 1; }

HOST_KEY="$(host_api /status | json "d['public_key']")"
PHONE_KEY="$(phone_api /status | json "d['public_key']")"
[[ -n "$HOST_KEY" && -n "$PHONE_KEY" ]] || { log "could not read node public keys from /api/status"; exit 1; }
log "host key:  ${HOST_KEY:0:16}…"
log "phone key: ${PHONE_KEY:0:16}…"

# ── Stages ────────────────────────────────────────────────────────────────────
# hears <api> <key>: that node has received <key>'s multicast announcements
# (a peer learned only from a handshake doesn't count).
hears() {
    "$1" /peers |
        json "any(p['public_key']=='$2' and p.get('discovered_via')=='multicast' for p in d['peers'])" |
        grep -q True
}
has_session() { "$1" /status | json "any(s['peer_pubkey']=='$2' for s in d['sessions'])" | grep -q True; }

send_msg() { # send_msg <api> <to> <text>
    "$1" /messages/send -X POST -H 'content-type: application/json' \
        -d "{\"to\":\"$2\",\"text\":\"$3\"}" >/dev/null
}
got_msg() { # got_msg <api> <from> <text>
    "$1" "/messages/$2" | json "any(m['content'].get('text')=='$3' for m in d['messages'])" | grep -q True
}

both_sessions() { has_session host_api "$PHONE_KEY" && has_session phone_api "$HOST_KEY"; }

run_session_stages() { # run_session_stages <suffix>
    local sfx="$1" nonce="$RANDOM$RANDOM"
    stage "session$sfx" "Noise session established (both sides)$sfx" both_sessions
    send_msg host_api "$PHONE_KEY" "h2p-$nonce"
    stage "msg_h2p$sfx" "message host -> phone$sfx" got_msg phone_api "$HOST_KEY" "h2p-$nonce"
    send_msg phone_api "$HOST_KEY" "p2h-$nonce"
    stage "msg_p2h$sfx" "message phone -> host$sfx" got_msg host_api "$PHONE_KEY" "p2h-$nonce"
}

log ""
log "stages:"
MISS=WARN stage phone_hears "phone hears host announcements (inbound multicast)" hears phone_api "$HOST_KEY"
MISS=WARN stage host_hears "host hears phone announcements (outbound multicast)" hears host_api "$PHONE_KEY"
run_session_stages ""

head -c 1048576 /dev/urandom >"$OUT/payload.bin"
want_hash="$(sha256sum "$OUT/payload.bin" | cut -d' ' -f1)"
"$HOST_BIN_DIR/summit-ctl" --port "$HOST_API" send "$OUT/payload.bin" --peer "$PHONE_KEY" >>"$OUT/smoke.log" 2>&1
file_arrived() {
    [[ "$("$ADB" shell "sha256sum $PHONE_DIR/home/received/payload.bin 2>/dev/null" | cut -d' ' -f1)" == "$want_hash" ]]
}
STAGE_TIMEOUT=$((STAGE_TIMEOUT * 2)) stage file "1 MiB file host -> phone (hash verified)" file_arrived

if ((SCREEN_OFF > 0)); then
    log ""
    log "screen off for ${SCREEN_OFF}s (Doze / Wi-Fi power-save filters)…"
    "$ADB" shell input keyevent KEYCODE_SLEEP
    sleep "$SCREEN_OFF"
    MISS=WARN stage still_hears " -> phone still hears host with screen off" hears phone_api "$HOST_KEY"
    run_session_stages " [screen off]"
    "$ADB" shell input keyevent KEYCODE_WAKEUP
fi

# ── Diagnosis ─────────────────────────────────────────────────────────────────
log ""
log "diagnosis:"
ph=${RESULT[phone_hears]} hh=${RESULT[host_hears]} se=${RESULT[session]}
if [[ $ph == WARN && $hh == WARN ]]; then
    log "  No multicast either way, so neither side can find the other. Wrong interface,"
    log "  IPv6 disabled, or the AP blocks multicast/client-to-client traffic (guest,"
    log "  public, mesh networks). Try the phone's hotspot or USB tethering."
elif [[ $se == FAIL ]]; then
    log "  At least one side discovered the other but no session formed. If only one side"
    log "  hears multicast, the one-way fallback should have kicked in within ~10 s: check"
    log "  the logs for 'initiating as fallback' / 'learned from handshake'. Otherwise"
    log "  unicast to link-local is blocked (host firewall on UDP? AP isolation?)."
elif [[ $ph == WARN ]]; then
    log "  Working, degraded: the phone doesn't receive multicast (Android filters it unless"
    log "  an app holds a WifiManager.MulticastLock; adb shell can't). It learned the host"
    log "  from the handshake, so it can only find peers that can hear IT. The app fixes this."
elif [[ $hh == WARN ]]; then
    log "  Working, degraded: the phone's multicast doesn't reach the host (AP filtering"
    log "  outbound multicast from this client?). Sessions still form via fallback."
elif [[ ${RESULT[file]} == FAIL && ${RESULT[msg_h2p]} == PASS ]]; then
    log "  Small chunks work, bulk doesn't: suspect MTU/fragmentation or rate limits."
elif [[ "${RESULT[*]}" != *FAIL* ]]; then
    log "  All stages passed."
else
    log "  Mixed result — read the stage list above and the logs."
fi

[[ "${RESULT[*]}" != *FAIL* ]]
