#!/usr/bin/env bash
# Creates two isolated network namespaces connected by a veth pair.
# summit-a <--> veth-a <--> veth-b <--> summit-b
#
# Run as root. Idempotent — safe to run twice.

set -euo pipefail

NS_A="summit-a"
NS_B="summit-b"
VETH_A="veth-a"
VETH_B="veth-b"

# Create namespaces if they don't exist
ip netns add "$NS_A" 2>/dev/null || echo "netns $NS_A already exists, skipping"
ip netns add "$NS_B" 2>/dev/null || echo "netns $NS_B already exists, skipping"

# Create veth pair if it doesn't exist
if ! ip link show "$VETH_A" &>/dev/null; then
    ip link add "$VETH_A" type veth peer name "$VETH_B"
    echo "Created veth pair $VETH_A <--> $VETH_B"
else
    echo "veth pair already exists, skipping"
fi

# Move each end into its namespace
ip link set "$VETH_A" netns "$NS_A"
ip link set "$VETH_B" netns "$NS_B"

# Bring up loopback and veth in each namespace
ip netns exec "$NS_A" ip link set lo up
ip netns exec "$NS_A" ip link set "$VETH_A" up

ip netns exec "$NS_B" ip link set lo up
ip netns exec "$NS_B" ip link set "$VETH_B" up

# IPv6 link-local addresses are assigned automatically when the interface
# comes up — no manual address configuration needed.
# Allow a moment for the kernel to assign them.
sleep 1

echo ""
echo "Network namespaces ready."
echo ""
echo "summit-a interface:"
ip netns exec "$NS_A" ip addr show "$VETH_A"
echo ""
echo "summit-b interface:"
ip netns exec "$NS_B" ip addr show "$VETH_B"
echo ""
echo "Use ./scripts/netns-run.sh <summit-a|summit-b> <command> to run inside a namespace."
