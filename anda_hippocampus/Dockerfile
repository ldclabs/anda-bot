# syntax=docker/dockerfile:1

FROM rust:slim-bookworm AS builder

RUN apt-get update \
    && apt-get install -y --no-install-recommends \
        gcc \
        g++ \
        libc6-dev \
        pkg-config \
        libssl-dev \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /build
COPY . .

RUN cargo build --release -p anda_hippocampus

FROM debian:bookworm-slim AS runtime

RUN apt-get update \
    && apt-get install -y --no-install-recommends \
        ca-certificates \
        tzdata \
        openssl \
    && update-ca-certificates \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /app

COPY --from=builder /build/target/release/anda_hippocampus /usr/local/bin/anda_hippocampus

ENV LISTEN_ADDR=0.0.0.0:8042

EXPOSE 8042

ENTRYPOINT ["/usr/local/bin/anda_hippocampus"]
CMD []