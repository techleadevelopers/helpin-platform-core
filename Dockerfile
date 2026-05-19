FROM rust:1-bookworm AS builder

WORKDIR /app

# Cache dependency compilation before copying the full source tree.
COPY Cargo.toml Cargo.lock ./
RUN mkdir src && \
    printf 'fn main() {}\n' > src/main.rs && \
    cargo build --release && \
    rm -rf src target/release/deps/zoohelp_backend*

COPY src ./src
COPY migrations ./migrations

RUN cargo build --release

FROM debian:bookworm-slim AS runtime

RUN apt-get update && \
    apt-get install -y --no-install-recommends ca-certificates && \
    rm -rf /var/lib/apt/lists/*

WORKDIR /app

COPY --from=builder /app/target/release/zoohelp-backend /app/zoohelp-backend
COPY --from=builder /app/migrations /app/migrations

ENV RUST_LOG=zoohelp_backend=info,tower_http=info

EXPOSE 8080

CMD ["sh", "-c", "if [ -z \"$BIND_ADDR\" ]; then export BIND_ADDR=\"0.0.0.0:${PORT:-8080}\"; fi; exec /app/zoohelp-backend"]
