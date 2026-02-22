# ── Build stage ────────────────────────────────────────────────────────────────
FROM rust:slim-bookworm AS builder

RUN apt-get update && \
    apt-get install -y --no-install-recommends \
        pkg-config \
        libssl-dev \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /build
COPY . .

RUN cargo build --release -p summitd -p summit-ctl && \
    strip target/release/summitd target/release/summit-ctl

# ── Runtime stage ──────────────────────────────────────────────────────────────
FROM debian:bookworm-slim

RUN apt-get update && \
    apt-get install -y --no-install-recommends \
        iproute2 \
        iputils-ping \
        ca-certificates \
    && rm -rf /var/lib/apt/lists/*

COPY --from=builder /build/target/release/summitd   /usr/local/bin/summitd
COPY --from=builder /build/target/release/summit-ctl /usr/local/bin/summit-ctl

# REST API
EXPOSE 9001

ENTRYPOINT ["summitd"]
# Default interface; override at runtime: docker run ... summit eth0
CMD ["eth0"]
