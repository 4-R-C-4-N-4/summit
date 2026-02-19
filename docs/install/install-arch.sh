#!/bin/bash
# Summit Protocol - Arch Linux Setup
# Installs dependencies and Summit from GitHub releases
set -e

VERSION="${1:-latest}"
REPO="4-R-C-4-N-4/summit"

GREEN='\033[0;32m'
YELLOW='\033[1;33m'
RED='\033[0;31m'
NC='\033[0m'

echo -e "${GREEN}━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━${NC}"
echo -e "${GREEN}Summit Protocol - Arch Linux Setup${NC}"
echo -e "${GREEN}━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━${NC}"
echo ""

if [ "$EUID" -ne 0 ]; then 
    echo -e "${RED}Please run as root (sudo ./install-arch.sh)${NC}"
    exit 1
fi

# Step 1: Update system
echo -e "${YELLOW}[1/4] Updating system...${NC}"
pacman -Sy --noconfirm

# Step 2: Install dependencies
echo -e "${YELLOW}[2/4] Installing dependencies...${NC}"
pacman -S --noconfirm --needed \
    curl \
    tar \
    iproute2 \
    iw \
    wireless_tools \
    wpa_supplicant \
    jq

# Step 3: Download and install Summit
echo -e "${YELLOW}[3/4] Installing Summit $VERSION...${NC}"

ARCH=$(uname -m)
case $ARCH in
    x86_64) ASSET_NAME="summit-x86_64-unknown-linux-gnu.tar.gz" ;;
    aarch64|arm64) ASSET_NAME="summit-aarch64-unknown-linux-gnu.tar.gz" ;;
    *) echo -e "${RED}Unsupported architecture: $ARCH${NC}"; exit 1 ;;
esac

DOWNLOAD_URL="https://github.com/$REPO/releases/download/$VERSION/$ASSET_NAME"
if [ "$VERSION" = "latest" ]; then
    DOWNLOAD_URL="https://github.com/$REPO/releases/latest/download/$ASSET_NAME"
fi

TEMP_DIR=$(mktemp -d)
cd "$TEMP_DIR"
curl -L -o "$ASSET_NAME" "$DOWNLOAD_URL"
tar xzf "$ASSET_NAME"
mv summitd /usr/local/bin/
mv summit-ctl /usr/local/bin/
chmod +x /usr/local/bin/summitd
chmod +x /usr/local/bin/summit-ctl
cd /
rm -rf "$TEMP_DIR"

# Step 4: Setup systemd service
echo -e "${YELLOW}[4/4] Creating systemd service...${NC}"

# Create WiFi detection script
cat > /usr/local/bin/summit-detect-wifi << 'EOF'
#!/bin/bash
WIFI=$(ip link show | grep -oP 'wl[a-z0-9]+' | head -1)
if [ -n "$WIFI" ]; then echo "$WIFI"; exit 0; fi
WIFI=$(iw dev | awk '$1=="Interface"{print $2}' | head -1)
if [ -n "$WIFI" ]; then echo "$WIFI"; exit 0; fi
echo "ERROR: No WiFi interface" >&2; exit 1
EOF
chmod +x /usr/local/bin/summit-detect-wifi

# Create systemd service
cat > /etc/systemd/system/summit.service << 'EOF'
[Unit]
Description=Summit Protocol Daemon
After=network.target

[Service]
Type=simple
ExecStartPre=/usr/local/bin/summit-detect-wifi
ExecStart=/bin/bash -c '/usr/local/bin/summitd $(/usr/local/bin/summit-detect-wifi)'
Restart=on-failure
RestartSec=5

[Install]
WantedBy=multi-user.target
EOF

systemctl daemon-reload
mkdir -p /tmp/summit-received
chmod 1777 /tmp/summit-received

echo ""
echo -e "${GREEN}✓ Installation complete!${NC}"
echo ""
echo "Quick start:"
echo "  sudo systemctl start summit"
echo "  summit-ctl status"
echo ""
echo "Web UI: http://127.0.0.1:9001"
