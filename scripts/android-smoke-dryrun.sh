#!/usr/bin/env bash
# Dry-run android-smoke.sh against a fake phone (a network namespace), rootless.
#
# Run this after changing android-smoke.sh, so that the next session with a
# real phone fails only for real reasons (the phone, the network), never
# because the test script itself is broken.
#
# Usage: scripts/android-smoke-dryrun.sh [--filter-multicast] [smoke args...]
#   --filter-multicast  make the fake phone drop inbound multicast, the way
#                       Android Wi-Fi does without a MulticastLock. Checks the
#                       failure diagnosis, not just the happy path.
set -euo pipefail
cd "$(dirname "$0")/.."

filter=0
if [[ "${1:-}" == "--filter-multicast" ]]; then
    filter=1
    shift
fi

target_dir="${CARGO_TARGET_DIR:-$PWD/target}"
cargo build -p summitd -p summit-ctl

phone_dir="$(mktemp -d "${TMPDIR:-/tmp}/fake-phone.XXXXXX")"

# Runs inside the namespaces.
inner="
set -e
if [[ $filter == 1 ]]; then
    ip netns exec summit-b nft add table inet android
    ip netns exec summit-b nft add chain inet android in '{ type filter hook input priority 0; }'
    ip netns exec summit-b nft add rule inet android in ip6 daddr ff02::1 udp dport 9000 drop
fi
exec ip netns exec summit-a env \
    ADB='$PWD/scripts/testing/fake-adb' \
    FAKE_ADB_NETNS=summit-b \
    PHONE_DIR='$phone_dir' \
    PHONE_BIN_DIR='$target_dir/debug' \
    HOST_BIN_DIR='$target_dir/debug' \
    scripts/android-smoke.sh --host-iface veth-a --phone-iface veth-b \"\$@\"
"
exec scripts/rootless-netns.sh bash -c "$inner" bash "$@"
