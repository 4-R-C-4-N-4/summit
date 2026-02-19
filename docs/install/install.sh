#!/bin/bash
# Summit Protocol - Universal Linux Installer
# Downloads and installs Summit from GitHub releases
set -e

VERSION="${1:-latest}"
REPO="4-R-C-4-N-4/summit"
INSTALL_DIR="/usr/local/bin"

# Colors
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
RED='\033[0;31m'
NC='\033[0m'

# Detect architecture
ARCH=$(uname -m)
case $ARCH in
    x86_64)
        ASSET_NAME="summit-x86_64-unknown-linux-gnu.tar.gz"
        ;;
    aarch64|arm64)
        ASSET_NAME="summit-aarch64-unknown-linux-gnu.tar.gz"
        ;;
    *)
        echo -e "${RED}Unsupported architecture: $ARCH${NC}"
        echo "Summit requires x86_64 or aarch64/arm64"
        exit 1
        ;;
esac

echo -e "${GREEN}━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━${NC}"
echo -e "${GREEN}Summit Protocol Installer${NC}"
echo -e "${GREEN}━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━${NC}"
echo ""
echo "Architecture: $ARCH"
echo "Version:      $VERSION"
echo ""

# Check if running as root
if [ "$EUID" -ne 0 ]; then 
    echo -e "${RED}Please run as root (sudo ./install.sh)${NC}"
    exit 1
fi

# Check for required tools
if ! command -v curl &> /dev/null && ! command -v wget &> /dev/null; then
    echo -e "${RED}Error: curl or wget is required${NC}"
    exit 1
fi

if ! command -v tar &> /dev/null; then
    echo -e "${RED}Error: tar is required${NC}"
    exit 1
fi

# Download binary
echo -e "${YELLOW}Downloading Summit $VERSION...${NC}"

DOWNLOAD_URL="https://github.com/$REPO/releases/download/$VERSION/$ASSET_NAME"
if [ "$VERSION" = "latest" ]; then
    DOWNLOAD_URL="https://github.com/$REPO/releases/latest/download/$ASSET_NAME"
fi

TEMP_DIR=$(mktemp -d)
cd "$TEMP_DIR"

if command -v curl &> /dev/null; then
    curl -L -o "$ASSET_NAME" "$DOWNLOAD_URL"
elif command -v wget &> /dev/null; then
    wget -O "$ASSET_NAME" "$DOWNLOAD_URL"
fi

if [ ! -f "$ASSET_NAME" ]; then
    echo -e "${RED}Download failed${NC}"
    exit 1
fi

# Extract
echo -e "${YELLOW}Extracting...${NC}"
tar xzf "$ASSET_NAME"

# Install binaries
echo -e "${YELLOW}Installing binaries...${NC}"
mv summitd "$INSTALL_DIR/"
mv summit-ctl "$INSTALL_DIR/"
chmod +x "$INSTALL_DIR/summitd"
chmod +x "$INSTALL_DIR/summit-ctl"

# Cleanup
cd /
rm -rf "$TEMP_DIR"

# Create received files directory
mkdir -p /tmp/summit-received
chmod 1777 /tmp/summit-received

echo ""
echo -e "${GREEN}━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━${NC}"
echo -e "${GREEN}✓ Installation complete!${NC}"
echo -e "${GREEN}━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━${NC}"
echo ""
echo "Binaries installed:"
echo "  $INSTALL_DIR/summitd"
echo "  $INSTALL_DIR/summit-ctl"
echo ""
echo "Quick start:"
echo "  1. Find your WiFi interface:"
echo "     ip link show | grep wl"
echo ""
echo "  2. Start Summit:"
echo "     sudo summitd wlp5s0  # Replace with your interface"
echo ""
echo "  3. In another terminal:"
echo "     summit-ctl status"
echo "     summit-ctl peers"
echo ""
echo "Access Web UI: http://127.0.0.1:9001"
echo ""
echo "For systemd setup, see: https://github.com/$REPO"
echo ""
