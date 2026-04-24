# engine-rust-v2/Dockerfile
# Multi-stage Dockerfile for building and running the Rust engine (Debian Bullseye)

# --- Builder stage ---------------------------------------------------------
FROM rust:1.70-slim-bullseye AS builder

# Install system dependencies required to build crates (OpenSSL, libzmq)
RUN apt-get update \
    && apt-get install -y --no-install-recommends \
       build-essential pkg-config libssl-dev libzmq3-dev ca-certificates git \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /usr/src/app

# Copy Cargo files first to leverage Docker layer caching for dependencies
COPY Cargo.toml Cargo.lock ./

# Create a dummy main to allow cargo to fetch/build deps (speeds up rebuilds)
RUN mkdir -p src && echo "fn main() { println!(\"dummy\"); }" > src/main.rs

# Fetch and build dependencies (may fail for workspace setups but generally caches deps)
RUN cargo build --release || true

# Copy the rest of the source
COPY . .

# Build the release binary
RUN cargo build --release


# --- Runtime stage ---------------------------------------------------------
FROM debian:bullseye-slim

# Install runtime dependencies (libzmq, SSL, ca-certificates)
RUN apt-get update \
    && apt-get install -y --no-install-recommends \
       libzmq3 ca-certificates libssl1.1 \
    && rm -rf /var/lib/apt/lists/*

# Create a non-root user to run the app
RUN useradd -m -s /bin/bash appuser
WORKDIR /home/appuser

# Copy the compiled binary from builder
COPY --from=builder /usr/src/app/target/release/engine-rust-v2 ./engine-rust-v2

# Ensure the binary is executable
RUN chmod +x ./engine-rust-v2 && chown appuser:appuser ./engine-rust-v2

USER appuser

# Expose ZeroMQ and notify ports used by the engine and python bridge
EXPOSE 5555 5556 5557

# Default environment for runtime logging level (can be overridden at docker run)
ENV RUST_LOG=debug

# Default entrypoint and recommended runtime arguments tailored to your command.
# You can still override any arguments when running the container (docker run <image> -- <args>).
ENTRYPOINT ["./engine-rust-v2"]
CMD ["--trade", "--symbol", "GOLD", "--min-score", "1", "--volume", "0.5", "--max-volume-per-trade", "10", "--max-total-volume", "50", "--sl-mult", "6", "--tp-mult", "12", "--cooldown-sec", "5", "--slack-enabled", "--slack-webhook", "https://hooks.slack.com/services/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", "--slack-channel", "ai-trading", "--slack-notify-port", "5557", "--auto-optimize", "--auto-reload-optimized-config", "--optimizer-output-file", "optimizer_result.json", "--optimizer-reload-sec", "120", "--history-count", "1000", "--use-mt5-bridge", "--mt5-bridge-script", "../python_bridge/main.py", "--mt5-symbol", "GOLD", "--log-level", "debug", "--require-history", "--history-wait-sec", "60"]
