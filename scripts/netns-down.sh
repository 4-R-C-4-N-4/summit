#!/usr/bin/env bash
# Tears down the Summit network namespace environment.
# Idempotent — safe to run twice.

set -euo pipefail

NS_A="summit-a"
NS_B="summit-b"

ip netns del "$NS_A" 2>/dev/null || echo "netns $NS_A not found, skipping"
ip netns del "$NS_B" 2>/dev/null || echo "netns $NS_B not found, skipping"

# veth interfaces are deleted automatically when their namespace is deleted
echo "Network namespaces torn down."
