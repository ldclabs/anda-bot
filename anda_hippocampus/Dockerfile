# base image
FROM rust:slim-bookworm AS builder

RUN apt-get update \
    && apt-get install -y gcc g++ libc6-dev pkg-config libssl-dev wget

WORKDIR /build
COPY anda_hippocampus ./anda_hippocampus
COPY Cargo.lock ./
COPY Cargo.toml ./
RUN cargo build --release -p anda_hippocampus

FROM debian:bookworm-slim AS runtime

# install dependency tools
RUN apt-get update \
    && apt-get install -y net-tools iptables iproute2 ca-certificates tzdata openssl \
    && update-ca-certificates \
    && rm -rf /var/lib/apt/lists/*

# working directory
WORKDIR /app

# your custom setup goes here
COPY --from=builder /build/target/release/anda_hippocampus ./anda_hippocampus

EXPOSE 8080
# entry point
ENTRYPOINT [ "/app/anda_hippocampus" ]