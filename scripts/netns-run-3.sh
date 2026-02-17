#!/bin/bash
set -e

# Create 3 network namespaces for multipath testing
NAMESPACES=("summit-a" "summit-b" "summit-c")
VETHS=("veth-a" "veth-b" "veth-c")

# Setup function (only run once)
setup() {
    # Cleanup any existing setup
    for ns in "${NAMESPACES[@]}"; do
        ip netns del "$ns" 2>/dev/null || true
    done
    ip link del br-summit 2>/dev/null || true

    # Create namespaces
    for ns in "${NAMESPACES[@]}"; do
        ip netns add "$ns"
    done

    # Create bridge
    ip link add br-summit type bridge
    ip link set br-summit up

    # Create veth pairs and connect to bridge
    for i in {0..2}; do
        ns="${NAMESPACES[$i]}"
        veth="${VETHS[$i]}"
        veth_br="${veth}-br"

        # Create veth pair
        ip link add "$veth" type veth peer name "$veth_br"

        # Move one end to namespace
        ip link set "$veth" netns "$ns"

        # Connect other end to bridge
        ip link set "$veth_br" master br-summit
        ip link set "$veth_br" up

        # Configure interface in namespace
        ip netns exec "$ns" ip link set lo up
        ip netns exec "$ns" ip link set "$veth" up
        ip netns exec "$ns" ip addr add "fd00::$((i+1))/64" dev "$veth"
    done

    echo "Network namespaces ready:"
    for i in {0..2}; do
        ns="${NAMESPACES[$i]}"
        echo "  $ns: fd00::$((i+1))"
    done

    # Create marker file
    touch /tmp/summit-3ns-ready
}

# Only setup if not already done
if [ ! -f /tmp/summit-3ns-ready ]; then
    echo "Setting up network..."
    setup
else
    echo "Network already setup, reusing..."
fi

# Run the command in the specified namespace
NS="$1"
shift
exec ip netns exec "$NS" "$@"
