#!/bin/bash
set -e

echo "Creating network namespaces..."

# Cleanup any existing setup
sudo ip netns del summit-a 2>/dev/null || true
sudo ip netns del summit-b 2>/dev/null || true

# Create namespaces
sudo ip netns add summit-a
sudo ip netns add summit-b

# Create veth pair
sudo ip link add veth-a type veth peer name veth-b

# Move to namespaces
sudo ip link set veth-a netns summit-a
sudo ip link set veth-b netns summit-b

# Configure summit-a
sudo ip netns exec summit-a ip link set lo up
sudo ip netns exec summit-a ip link set veth-a up
sudo ip netns exec summit-a ip addr add fd00::1/64 dev veth-a

# Configure summit-b
sudo ip netns exec summit-b ip link set lo up
sudo ip netns exec summit-b ip link set veth-b up
sudo ip netns exec summit-b ip addr add fd00::2/64 dev veth-b

echo "Network namespaces ready!"
echo "  summit-a: fd00::1 (veth-a)"
echo "  summit-b: fd00::2 (veth-b)"
echo ""
echo "Run daemons with:"
echo "  Terminal 1: sudo RUST_LOG=info ./scripts/netns-run.sh summit-a ./target/debug/summitd veth-a"
echo "  Terminal 2: sudo RUST_LOG=info ./scripts/netns-run.sh summit-b ./target/debug/summitd veth-b"
