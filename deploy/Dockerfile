# ── Stage 1: Build Rust binaries ──────────────────────────────
FROM rust:slim-bookworm AS builder

RUN apt-get update && apt-get install -y \
  pkg-config libssl-dev musl-tools \
  && rm -rf /var/lib/apt/lists/*

WORKDIR /app
COPY Cargo.toml Cargo.lock ./

# Build dependencies first (cached layer)
RUN mkdir -p src/bin && echo "fn main() {}" > src/main.rs && echo "fn main() {}" > src/bin/ws-server.rs
RUN cargo build --release --bins 2>/dev/null || true

# Full build
COPY src/ src/
RUN cargo build --release --bins

# ── Stage 2: Runtime image ───────────────────────────────────
FROM debian:bookworm-slim

RUN apt-get update && apt-get install -y \
  ca-certificates curl git \
  && rm -rf /var/lib/apt/lists/*

# Create a non-root user
RUN groupadd -g 1001 codeagent && \
  useradd -m -u 1001 -g codeagent -s /bin/bash codeagent

WORKDIR /workspace

# Copy binaries from builder
COPY --from=builder /app/target/release/my-code-agent /usr/local/bin/
COPY --from=builder /app/target/release/ws-server /usr/local/bin/

# Copy default config
COPY deploy/config.toml /etc/my-code-agent/config.toml

# Create runtime data directory
RUN mkdir -p /workspace/.mycode && chown -R codeagent:codeagent /workspace

USER codeagent

ENV MY_CODE_AGENT_HOME=/workspace/.mycode

EXPOSE 8088 8089

# Default: run ws-server (override with docker-compose or CMD)
CMD ["ws-server"]