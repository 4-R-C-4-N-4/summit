#!/usr/bin/env bash
# Run a command inside a throwaway two-node network, without sudo.
#
# Creates a user + mount + network namespace (unshare -r), mounts a private
# /run so `ip netns add` works, and builds the summit-a/summit-b veth pair
# that netns-up.sh builds with sudo:
#
#     [summit-a] veth-a <=======> veth-b [summit-b]
#
# Then runs "$@" in the outer namespace (use `ip netns exec summit-a …` to
# act as a node). Nothing touches the host network; it all disappears on exit.
# HOME is pointed at a scratch dir so daemons never see your real keypair or
# trust store.
#
# Needs unprivileged user namespaces (default on Arch/Fedora/Debian; Ubuntu
# 24.04+ restricts them via AppArmor — use netns-up.sh with sudo there).
#
# Usage: scripts/rootless-netns.sh <command> [args...]
set -euo pipefail

if [[ "${SUMMIT_ROOTLESS_INSIDE:-}" != 1 ]]; then
    export SUMMIT_ROOTLESS_INSIDE=1
    exec unshare -r -m -n --propagation private "$0" "$@"
fi

mount -t tmpfs tmpfs /run
mkdir -p /run/netns

ip netns add summit-a
ip netns add summit-b
ip link add veth-a type veth peer name veth-b
ip link set veth-a netns summit-a
ip link set veth-b netns summit-b
for ns in summit-a summit-b; do
    ip netns exec "$ns" ip link set lo up
done
ip netns exec summit-a ip link set veth-a up
ip netns exec summit-b ip link set veth-b up

# Wait for IPv6 DAD so link-local addresses are usable.
for _ in $(seq 1 50); do
    if ! ip netns exec summit-a ip -6 addr show veth-a | grep -q tentative &&
        ! ip netns exec summit-b ip -6 addr show veth-b | grep -q tentative; then
        break
    fi
    sleep 0.1
done

# Anything left running in the namespaces (daemons, forwarders) dies with us,
# so stray processes can't keep pipes open or ports bound after we return.
reap() {
    for ns in summit-a summit-b; do
        ip netns pids "$ns" 2>/dev/null | xargs -r kill -9 2>/dev/null || true
    done
}
trap reap EXIT

scratch_home="$(mktemp -d)"
export CARGO_HOME="${CARGO_HOME:-$HOME/.cargo}"
export RUSTUP_HOME="${RUSTUP_HOME:-$HOME/.rustup}"
export HOME="$scratch_home"
unset XDG_CONFIG_HOME XDG_DATA_HOME

"$@"
