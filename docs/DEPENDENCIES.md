# Summit Protocol - Dependencies Guide

## Arch Linux

### System Requirements
- **OS:** Arch Linux (rolling release)
- **Kernel:** 5.10+ (for io_uring support)
- **Architecture:** x86_64 or aarch64
- **RAM:** 512MB minimum, 1GB recommended
- **Disk:** 100MB for binaries, cache grows with usage

### Core Dependencies

```bash
sudo pacman -S --noconfirm \
    base-devel \      # Build tools (gcc, make, etc.)
    git \             # Version control
    rustup \          # Rust toolchain manager
    nodejs \          # Node.js runtime (for UI build)
    npm               # Node package manager
```

### Network Dependencies

```bash
sudo pacman -S --noconfirm \
    iproute2 \        # ip command for network config
    iw \              # Wireless configuration
    wireless_tools \  # iwconfig, iwlist
    wpa_supplicant    # WPA/WPA2 authentication
```

### Optional Dependencies

```bash
sudo pacman -S --noconfirm \
    jq \              # JSON parsing (for CLI scripts)
    tcpdump \         # Packet capture (debugging)
    wireshark-cli     # Network analysis
```

### Rust Setup

```bash
# Install stable Rust toolchain
rustup default stable

# Add components
rustup component add clippy rustfmt rust-analyzer
```

---

## Ubuntu/Debian

### System Requirements
Same as Arch Linux

### Core Dependencies

```bash
sudo apt update
sudo apt install -y \
    build-essential \
    git \
    curl \
    pkg-config \
    libssl-dev \
    nodejs \
    npm
```

### Network Dependencies

```bash
sudo apt install -y \
    iproute2 \
    wireless-tools \
    iw \
    wpasupplicant
```

### Rust Setup

```bash
# Install Rust via rustup
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
source $HOME/.cargo/env

# Add components
rustup component add clippy rustfmt
```

---

## Fedora/RHEL

### Core Dependencies

```bash
sudo dnf install -y \
    gcc \
    gcc-c++ \
    make \
    git \
    openssl-devel \
    nodejs \
    npm
```

### Network Dependencies

```bash
sudo dnf install -y \
    iproute \
    wireless-tools \
    iw \
    wpa_supplicant
```

### Rust Setup

```bash
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
source $HOME/.cargo/env
rustup component add clippy rustfmt
```

---

## macOS

### System Requirements
- **OS:** macOS 11 (Big Sur) or later
- **Architecture:** x86_64 or Apple Silicon (aarch64)

### Core Dependencies

```bash
# Install Homebrew if not present
/bin/bash -c "$(curl -fsSL https://raw.githubusercontent.com/Homebrew/install/HEAD/install.sh)"

# Install dependencies
brew install git node
```

### Rust Setup

```bash
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
source $HOME/.cargo/env
rustup component add clippy rustfmt
```

**Note:** WiFi Direct support on macOS is limited. Use standard WiFi interfaces.

---

## Runtime Dependencies

### Required Capabilities

Summit requires the following Linux capabilities:

- `CAP_NET_ADMIN` - Create network namespaces (testing only)
- `CAP_NET_RAW` - Raw socket access for link-local multicast

**Grant capabilities:**
```bash
sudo setcap cap_net_admin,cap_net_raw+ep /usr/local/bin/summitd
```

Or run as root (simpler for testing):
```bash
sudo summitd wlp5s0
```

### Network Configuration

Summit uses IPv6 link-local multicast for peer discovery:
- **Multicast group:** `ff02::1` (all nodes)
- **Ports:** 9000 (discovery), 9001 (HTTP API), ephemeral (chunk transfer)

**Firewall rules (if needed):**

```bash
# Allow multicast on interface
sudo ip6tables -A INPUT -i wlp5s0 -p udp --dport 9000 -j ACCEPT
sudo ip6tables -A INPUT -i wlp5s0 -p udp --dport 9001 -j ACCEPT

# Or disable firewall for testing
sudo systemctl stop firewalld  # Fedora/RHEL
sudo ufw disable               # Ubuntu
```

---

## Verification

After installation, verify everything works:

```bash
# Check binaries
which summitd
which summit-ctl

# Check versions
summitd --version  # Will error but confirms binary exists
summit-ctl --help

# Check Rust
rustc --version
cargo --version

# Check Node (if building UI)
node --version
npm --version

# Check network tools
ip -6 addr show
iw dev
```

---

## Build from Source

### Standard Build (no UI)

```bash
git clone https://github.com/yourorg/summit.git
cd summit
cargo build --release -p summitd
cargo build --release -p summit-ctl

# Install
sudo cp target/release/summitd /usr/local/bin/
sudo cp target/release/summit-ctl /usr/local/bin/
```

### Build with Embedded UI

```bash
# Build React app
cd astral
npm install
npm run build
cd ..

# Build Rust with embedded UI
cargo build --release --features embed-ui -p summitd
cargo build --release -p summit-ctl

# Install
sudo cp target/release/summitd /usr/local/bin/
sudo cp target/release/summit-ctl /usr/local/bin/
```

### Quick Build Script

```bash
./scripts/build-astral.sh  # Builds UI + daemon
./scripts/run-wifi.sh      # Run on WiFi interface
```

---

## Troubleshooting

### "Failed to bind to port 9000"
- Port already in use
- Solution: `sudo lsof -i :9000` and kill the process

### "No such device" error
- Interface name wrong
- Solution: `ip link show` to find correct name

### "Operation not permitted"
- Missing capabilities
- Solution: Run with `sudo` or grant capabilities

### "Cannot open network namespace"
- Namespace doesn't exist (testing only)
- Solution: `sudo ./scripts/netns-up.sh`

### Multicast not working
- IPv6 disabled or firewall blocking
- Solution: `sysctl net.ipv6.conf.all.disable_ipv6` should be 0

---

## WiFi Direct (GOAL-6)

For WiFi Direct P2P mode, additional dependencies:

```bash
# Arch
sudo pacman -S wpa_supplicant

# Ubuntu
sudo apt install wpasupplicant

# Configuration
sudo wpa_cli p2p_find
sudo wpa_cli p2p_connect <peer_mac> pbc
```

See `docs/WIFI_DIRECT.md` for full setup guide.

---

## Development Dependencies

For development and testing:

```bash
# Additional tools
sudo pacman -S --noconfirm \
    gdb \              # Debugger
    strace \           # System call tracer
    heaptrack \        # Memory profiler
    valgrind \         # Memory checker
    perf \             # Performance profiler
    tcpdump \          # Packet capture
    wireshark-cli      # Network analysis

# Cargo tools
cargo install cargo-watch    # Auto-rebuild on changes
cargo install cargo-audit    # Security auditing
cargo install flamegraph     # Performance profiling
```

---

## Container Development

Using Docker/Podman:

```bash
# Build container
docker build -t summit .

# Run with privileges (for network namespaces)
docker run -it --rm --privileged \
  -v $(pwd):/summit \
  summit bash

# Inside container
cd /summit
cargo build --release
./scripts/netns-up.sh
```

See `Dockerfile` for full container setup.
