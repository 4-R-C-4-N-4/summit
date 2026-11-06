#!/usr/bin/env bash
# Run the integration suite without sudo (see rootless-netns.sh).
#
# Usage: scripts/integration-rootless.sh [cargo test filter/args...]
#   scripts/integration-rootless.sh
#   scripts/integration-rootless.sh messaging:: -- --nocapture
set -euo pipefail
cd "$(dirname "$0")/.."

# Build outside the namespace (it has no network for cargo).
cargo build -p summitd -p summit-ctl
cargo test --test integration --no-run

exec scripts/rootless-netns.sh cargo test --offline --test integration "$@"
