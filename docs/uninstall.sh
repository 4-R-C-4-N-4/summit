#!/bin/bash
# Summit Protocol - Uninstall Script

set -e

GREEN='\033[0;32m'
YELLOW='\033[1;33m'
RED='\033[0;31m'
NC='\033[0m'

echo -e "${YELLOW}━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━${NC}"
echo -e "${YELLOW}Summit Protocol - Uninstall${NC}"
echo -e "${YELLOW}━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━${NC}"
echo ""

# Check if running as root
if [ "$EUID" -ne 0 ]; then 
    echo -e "${RED}Please run as root (sudo ./uninstall.sh)${NC}"
    exit 1
fi

# Stop and disable systemd service
echo -e "${YELLOW}[1/5] Stopping Summit service...${NC}"
systemctl stop summit 2>/dev/null || echo "  Service not running"
systemctl disable summit 2>/dev/null || echo "  Service not enabled"
rm -f /etc/systemd/system/summit.service
systemctl daemon-reload

# Remove binaries
echo -e "${YELLOW}[2/5] Removing binaries...${NC}"
rm -f /usr/local/bin/summitd
rm -f /usr/local/bin/summit-ctl
rm -f /usr/local/bin/summit-detect-wifi

# Remove cache and data
echo -e "${YELLOW}[3/5] Cleaning cache and data...${NC}"
rm -rf /tmp/summit-cache-*
rm -rf /tmp/summit-received

# Remove installation directory (if exists)
echo -e "${YELLOW}[4/5] Removing installation directory...${NC}"
if [ -d "/opt/summit" ]; then
    rm -rf /opt/summit
    echo "  ✓ Removed /opt/summit"
else
    echo "  No /opt/summit directory found"
fi

# Clear cargo cache (optional)
echo -e "${YELLOW}[5/5] Clearing build cache (optional)...${NC}"
ACTUAL_USER=$(logname 2>/dev/null || echo $SUDO_USER)
if [ -n "$ACTUAL_USER" ] && [ "$ACTUAL_USER" != "root" ]; then
    USER_HOME=$(eval echo ~$ACTUAL_USER)
    if [ -d "$USER_HOME/.cargo/registry" ]; then
        echo "  Build cache at $USER_HOME/.cargo (not removed)"
        echo "  To remove: rm -rf $USER_HOME/.cargo/registry/cache/*"
    fi
fi

echo ""
echo -e "${GREEN}━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━${NC}"
echo -e "${GREEN}✓ Summit uninstalled successfully${NC}"
echo -e "${GREEN}━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━${NC}"
echo ""
echo "Removed:"
echo "  • Summit daemon (summitd)"
echo "  • Summit CLI (summit-ctl)"
echo "  • Systemd service"
echo "  • Cache and received files"
echo ""
echo "To reinstall:"
echo "  sudo ./docs/install/install-arch.sh"
echo ""
