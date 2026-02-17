#!/bin/bash
set -e

echo "Cleaning up old namespaces..."
sudo ip netns del summit-a 2>/dev/null || true
sudo ip netns del summit-b 2>/dev/null || true
sudo ip netns del summit-c 2>/dev/null || true
sudo ip link del br-summit 2>/dev/null || true

echo "Creating 3 network namespaces..."

# Create namespaces
sudo ip netns add summit-a
sudo ip netns add summit-b
sudo ip netns add summit-c

# Create bridge
sudo ip link add br-summit type bridge
sudo ip link set br-summit up

# Create veth pairs and connect to bridge
for ns in summit-a summit-b summit-c; do
    veth="${ns#summit-}-veth"
    veth_br="${veth}-br"
    
    # Create veth pair
    sudo ip link add "$veth" type veth peer name "$veth_br"
    
    # Move one end to namespace
    sudo ip link set "$veth" netns "$ns"
    
    # Connect other end to bridge
    sudo ip link set "$veth_br" master br-summit
    sudo ip link set "$veth_br" up
    
    # Configure in namespace
    sudo ip netns exec "$ns" ip link set lo up
    sudo ip netns exec "$ns" ip link set "$veth" up
done

# Assign IPs
sudo ip netns exec summit-a ip addr add fd00::1/64 dev a-veth
sudo ip netns exec summit-b ip addr add fd00::2/64 dev b-veth
sudo ip netns exec summit-c ip addr add fd00::3/64 dev c-veth

echo "Network namespaces ready!"
echo "  summit-a: fd00::1 (a-veth)"
echo "  summit-b: fd00::2 (b-veth)"
echo "  summit-c: fd00::3 (c-veth)"
echo ""
echo "Run daemons with:"
echo "  Terminal 1: sudo ip netns exec summit-a env RUST_LOG=info ./target/debug/summitd a-veth"
echo "  Terminal 2: sudo ip netns exec summit-b env RUST_LOG=info ./target/debug/summitd b-veth"
echo "  Terminal 3: sudo ip netns exec summit-c env RUST_LOG=info ./target/debug/summitd c-veth"
