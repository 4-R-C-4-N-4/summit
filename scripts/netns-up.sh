#!/usr/bin/env bash
set -euo pipefail

NS_A="summit-a"
NS_B="summit-b"
VETH_A="veth-a"
VETH_B="veth-b"

# Force cleanup - remove namespace directories directly
rm -rf /var/run/netns/"$NS_A" /var/run/netns/"$NS_B" 2>/dev/null || true
ip link delete "$VETH_A" 2>/dev/null || true

# Create namespaces
ip netns add "$NS_A"
ip netns add "$NS_B"

# Create veth pair
ip link add "$VETH_A" type veth peer name "$VETH_B"
echo "Created veth pair $VETH_A <--> $VETH_B"

# Move each end into its namespace
ip link set "$VETH_A" netns "$NS_A"
ip link set "$VETH_B" netns "$NS_B"

# Bring up interfaces
ip netns exec "$NS_A" ip link set lo up
ip netns exec "$NS_A" ip link set "$VETH_A" up
ip netns exec "$NS_B" ip link set lo up
ip netns exec "$NS_B" ip link set "$VETH_B" up

sleep 1

echo ""
echo "Network namespaces ready."
echo ""
echo "summit-a interface:"
ip netns exec "$NS_A" ip addr show "$VETH_A"
echo ""
echo "summit-b interface:"
ip netns exec "$NS_B" ip addr show "$VETH_B"
