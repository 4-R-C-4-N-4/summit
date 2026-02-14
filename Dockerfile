FROM archlinux:latest

# System update
RUN pacman -Syu --noconfirm

# Core build dependencies
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
    vim

# Rust stable toolchain
RUN rustup toolchain install stable && \
    rustup component add clippy rustfmt rust-analyzer

# Cargo tools
RUN cargo install cbindgen cargo-audit cargo-watch

# Working directory
WORKDIR /summit

# Mount point for source — develop on host, build in container
VOLUME ["/summit"]
