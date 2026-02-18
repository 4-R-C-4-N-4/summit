FROM archlinux:latest

# Force mirror refresh and system update
RUN pacman -Syy --noconfirm && \
    pacman -Syu --noconfirm

# Core build dependencies + Node.js
RUN pacman -S --noconfirm \
    base-devel \
    rustup \
    liburing \
    git \
    iproute2 \
    iputils \
    tcpdump \
    wireshark-cli \
    wpa_supplicant \
    gdb \
    strace \
    heaptrack \
    vim \
    nodejs \
    npm \
    jq

# Rust stable toolchain
RUN rustup toolchain install stable && \
    rustup component add clippy rustfmt rust-analyzer

# Cargo tools
RUN cargo install cbindgen cargo-audit cargo-watch

# Working directory
WORKDIR /summit

# Mount point for source
VOLUME ["/summit"]
