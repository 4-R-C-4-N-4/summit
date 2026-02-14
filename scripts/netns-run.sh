#!/usr/bin/env bash
# Run a command inside a Summit network namespace.
#
# Usage: ./scripts/netns-run.sh <summit-a|summit-b> <command> [args...]
#
# Example:
#   ./scripts/netns-run.sh summit-a ping -6 -c 1 fe80::...%veth-a

set -euo pipefail

if [[ $# -lt 2 ]]; then
    echo "Usage: $0 <namespace> <command> [args...]" >&2
    exit 1
fi

NS="$1"
shift

if [[ "$NS" != "summit-a" && "$NS" != "summit-b" ]]; then
    echo "Error: namespace must be 'summit-a' or 'summit-b', got '$NS'" >&2
    exit 1
fi

exec ip netns exec "$NS" "$@"
