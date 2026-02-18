#!/bin/bash
set -e

echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
echo "Building Astral - Summit Protocol Mesh"
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"

# Build Astral UI
echo ""
echo "→ Building Astral UI..."
cd "$PROJECT_ROOT/astral"

if [ ! -d "node_modules" ]; then
    echo "  Installing npm dependencies..."
    npm install
fi

echo "  Running Vite build..."
npm run build

# Verify dist was created
if [ ! -d "dist" ]; then
    echo "ERROR: astral/dist directory not created!"
    exit 1
fi

echo "  ✓ UI built: $(du -sh dist | cut -f1)"

# Build summitd with embedded UI
echo ""
echo "→ Building summitd with embedded Astral UI..."
cd "$PROJECT_ROOT"
cargo build --release --features embed-ui -p summitd  # ADD --features embed-ui

echo ""
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
echo "✓ Build complete!"
echo ""
echo "Binary location:"
echo "  $PROJECT_ROOT/target/release/summitd"
echo ""
echo "To run (requires network namespace):"
echo "  sudo ./scripts/netns-up.sh"
echo "  sudo ip netns exec summit-a ./target/release/summitd veth-a"
echo ""
echo "Then open: http://127.0.0.1:9001"
echo "  API:  http://127.0.0.1:9001/api/status"
echo "  UI:   http://127.0.0.1:9001/"
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
