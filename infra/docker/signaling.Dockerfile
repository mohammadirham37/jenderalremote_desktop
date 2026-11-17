# Build multi-stage untuk signaling service (dev/staging).
# CI GitHub Actions membangun aplikasi desktop; image ini untuk server.
FROM rust:1.83-slim AS builder
WORKDIR /build
COPY Cargo.toml Cargo.lock ./
COPY crates ./crates
COPY services ./services
# apps/desktop tidak dibutuhkan di server; workspace tetap butuh path-nya
COPY apps/desktop/src-tauri ./apps/desktop/src-tauri
RUN cargo build --release -p jenderalremote-signaling

FROM debian:bookworm-slim
RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates wget \
    && rm -rf /var/lib/apt/lists/*
COPY --from=builder /build/target/release/jenderalremote-signaling /usr/local/bin/signaling
ENV BIND=0.0.0.0:8080 RUST_LOG=info
EXPOSE 8080
ENTRYPOINT ["/usr/local/bin/signaling"]
