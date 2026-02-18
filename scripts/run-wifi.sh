#!/bin/bash
set -e

# Colors for output
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
RED='\033[0;31m'
NC='\033[0m' # No Color

echo -e "${GREEN}━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━${NC}"
echo -e "${GREEN}Summit Protocol - WiFi Interface${NC}"
echo -e "${GREEN}━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━${NC}"


# Auto-detect WiFi interface
WIFI_IFACE=$(./scripts/detect-wifi.sh)

if [ $? -ne 0 ]; then
    echo -e "${RED}ERROR: $WIFI_IFACE${NC}"
    echo "Available interfaces:"
    ip link show
    exit 1
fi

echo -e "${YELLOW}WiFi Interface: ${WIFI_IFACE}${NC}"
echo ""

# Check if binary exists
if [ ! -f "./target/release/summitd" ]; then
    echo -e "${RED}ERROR: summitd binary not found${NC}"
    echo "Build it first: ./scripts/build-astral.sh"
    exit 1
fi

# Cleanup function
cleanup() {
    echo ""
    echo -e "${YELLOW}━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━${NC}"
    echo -e "${YELLOW}Cleaning up...${NC}"
    echo -e "${YELLOW}━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━${NC}"
    
    # Kill any running summitd processes
    pkill -f summitd || true
    
    # Clear cache
    rm -rf /tmp/summit-cache-* 2>/dev/null || true
    rm -rf /tmp/summit-received 2>/dev/null || true
    
    echo -e "${GREEN}✓ Cleanup complete${NC}"
}

# Trap Ctrl+C and other exit signals
trap cleanup EXIT INT TERM

echo -e "${GREEN}Starting Summit daemon...${NC}"
echo ""
echo "  Interface : ${WIFI_IFACE}"
echo "  API       : http://127.0.0.1:9001/api/status"
echo "  Web UI    : http://127.0.0.1:9001/"
echo ""
echo -e "${YELLOW}Press Ctrl+C to stop and cleanup${NC}"
echo ""
echo -e "${GREEN}━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━${NC}"
echo ""

# Run the daemon
sudo ./target/release/summitd "$WIFI_IFACE"
