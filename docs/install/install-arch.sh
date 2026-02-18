#!/bin/bash
# Summit Protocol - Arch Linux Installation Script
set -e

GREEN='\033[0;32m'
YELLOW='\033[1;33m'
RED='\033[0;31m'
NC='\033[0m'

echo -e "${GREEN}━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━${NC}"
echo -e "${GREEN}Summit Protocol - Arch Linux Setup${NC}"
echo -e "${GREEN}━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━${NC}"
echo ""

# Check if running as root
if [ "$EUID" -ne 0 ]; then 
    echo -e "${RED}Please run as root (sudo ./install-arch.sh)${NC}"
    exit 1
fi

# Step 1: Update system
echo -e "${YELLOW}[1/5] Updating system packages...${NC}"
pacman -Syu --noconfirm

# Step 2: Install system dependencies
echo -e "${YELLOW}[2/5] Installing system dependencies...${NC}"
pacman -S --noconfirm \
    base-devel \
    git \
    rustup \
    nodejs \
    npm \
    iproute2 \
    iw \
    wireless_tools \
    wpa_supplicant \
    jq

# Step 3: Setup Rust toolchain
echo -e "${YELLOW}[3/5] Setting up Rust toolchain...${NC}"
if ! command -v rustc &> /dev/null; then
    # Run as the actual user, not root
    ACTUAL_USER=$(logname 2>/dev/null || echo $SUDO_USER)
    if [ -n "$ACTUAL_USER" ] && [ "$ACTUAL_USER" != "root" ]; then
        sudo -u "$ACTUAL_USER" rustup default stable
        sudo -u "$ACTUAL_USER" rustup component add clippy rustfmt
    else
        rustup default stable
        rustup component add clippy rustfmt
    fi
else
    echo "  ✓ Rust already installed"
fi

# Step 4: Clone and build Summit
echo -e "${YELLOW}[4/5] Building Summit Protocol...${NC}"

INSTALL_DIR="/opt/summit"
ACTUAL_USER=$(logname 2>/dev/null || echo $SUDO_USER)

# If in a git repo, use current directory
if git rev-parse --git-dir > /dev/null 2>&1; then
    echo "  Using current directory (git repository detected)"
    PROJECT_DIR="$(pwd)"
else
    echo "  Cloning to $INSTALL_DIR..."
    rm -rf "$INSTALL_DIR"
    git clone https://github.com/4-R-C-4-N-4/summit.git "$INSTALL_DIR"
    PROJECT_DIR="$INSTALL_DIR"
fi

cd "$PROJECT_DIR"

# Build as actual user if possible
if [ -n "$ACTUAL_USER" ] && [ "$ACTUAL_USER" != "root" ]; then
    sudo -u "$ACTUAL_USER" cargo build --release -p summitd
    sudo -u "$ACTUAL_USER" cargo build --release -p summit-ctl
else
    cargo build --release -p summitd
    cargo build --release -p summit-ctl
fi

# Step 5: Install binaries
echo -e "${YELLOW}[5/5] Installing binaries...${NC}"
cp target/release/summitd /usr/local/bin/
cp target/release/summit-ctl /usr/local/bin/
chmod +x /usr/local/bin/summitd
chmod +x /usr/local/bin/summit-ctl

# Create systemd service with dynamic interface detection
cat > /etc/systemd/system/summit.service << 'SYSTEMD_EOF'
[Unit]
Description=Summit Protocol Daemon
After=network.target

[Service]
Type=simple
ExecStartPre=/usr/local/bin/summit-detect-wifi
ExecStart=/bin/bash -c '/usr/local/bin/summitd $(/usr/local/bin/summit-detect-wifi)'
Restart=on-failure
RestartSec=5
StandardOutput=journal
StandardError=journal

# Security hardening
NoNewPrivileges=true
PrivateTmp=true
ProtectSystem=strict
ProtectHome=true
ReadWritePaths=/tmp

[Install]
WantedBy=multi-user.target
SYSTEMD_EOF

# Install detection helper
cp scripts/detect-wifi.sh /usr/local/bin/summit-detect-wifi
chmod +x /usr/local/bin/summit-detect-wifi

# Create received files directory
mkdir -p /tmp/summit-received
chmod 777 /tmp/summit-received

echo ""
echo -e "${GREEN}━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━${NC}"
echo -e "${GREEN}✓ Installation complete!${NC}"
echo -e "${GREEN}━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━${NC}"
echo ""
echo "Binaries installed:"
echo "  /usr/local/bin/summitd"
echo "  /usr/local/bin/summit-ctl"
echo ""
echo "Quick start:"
echo "  1. Find your WiFi interface:     ip link show"
echo "  2. Edit systemd service:         sudo systemctl edit summit.service"
echo "     (Change wlp5s0 to your interface)"
echo "  3. Start Summit:                 sudo systemctl start summit"
echo "  4. Enable on boot:               sudo systemctl enable summit"
echo "  5. Check status:                 summit-ctl status"
echo ""
echo "Access Web UI: http://127.0.0.1:9001"
echo ""
