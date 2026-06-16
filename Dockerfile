FROM rust:1.82-slim AS builder

WORKDIR /app
COPY Cargo.toml Cargo.lock ./
COPY crates/ crates/

RUN cargo build --release -p aikd-cli

FROM debian:bookworm-slim

RUN apt-get update && apt-get install -y --no-install-recommends \
    ca-certificates \
    && rm -rf /var/lib/apt/lists/*

COPY --from=builder /app/target/release/aikd /usr/local/bin/aikd

RUN mkdir -p /root/.aikd

EXPOSE 9090

ENTRYPOINT ["aikd"]
CMD ["daemon", "--foreground"]
