# IronForge - Rust Git Hosting Platform
# Multi-stage build for minimal image size

# ── Stage 1: Builder ──────────────────────────────────
FROM rust:1.95-bookworm as builder

# Install system dependencies
RUN apt-get update && apt-get install -y \
    pkg-config \
    libsqlite3-dev \
    libssl-dev \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /app

# Copy dependency files first (leverage Docker cache)
COPY Cargo.toml Cargo.lock ./
COPY crates/*/Cargo.toml ./

# Create dummy files to build dependencies (trick to cache dependencies)
RUN mkdir -p crates/rg-cli/src \
    && echo 'fn main() {}' > crates/rg-cli/src/main.rs \
    && cargo build --release 2>&1 || true

# Copy source code
COPY . .

# Build the actual project
RUN cargo build --release

# ── Stage 2: Runtime ───────────────────────────────────
FROM debian:bookworm-slim

# Install runtime dependencies
RUN apt-get update && apt-get install -y \
    libsqlite3-0 \
    libssl3 \
    ca-certificates \
    git \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /data

# Create non-root user
RUN useradd -m -u 1000 ironforge && \
    mkdir -p /data/repos /data/logs && \
    chown -R ironforge:ironforge /data

USER ironforge

# Copy binary from builder
COPY --from=builder /app/target/release/ironforge /usr/local/bin/ironforge
COPY --from=builder /app/target/release/ironforge-runner /usr/local/bin/ironforge-runner
COPY --from=builder /app/target/release/ironforge-mcp /usr/local/bin/ironforge-mcp

# Expose ports
# 8080: HTTP
# 2222: SSH
# 3000: Runner (internal)
EXPOSE 8080 2222

# Environment variables (override with docker run -e)
ENV IRONFORGE_JWT_SECRET=""
ENV IRONFORGE_REPO_ROOT="/data/repos"
ENV IRONFORGE_DB_URL="sqlite:///data/ironforge.db?mode=rwc"
ENV IRONFORGE_HTTP_ADDR="0.0.0.0:8080"
ENV IRONFORGE_SSH_ADDR="0.0.0.0:2222"
ENV IRONFORGE_LOG_FILE="/data/logs/ironforge.log"
ENV IRONFORGE_LOG_MAX_SIZE_MB="100"
ENV IRONFORGE_LOG_MAX_FILES="7"

# Health check
HEALTHCHECK --interval=30s --timeout=5s --start-period=10s --retries=3 \
    CMD curl -f http://localhost:8080/health || exit 1

# Default command
CMD ["ironforge", "serve", \
    "--repo-root", "/data/repos", \
    "--http-addr", "0.0.0.0:8080", \
    "--ssh-addr", "0.0.0.0:2222", \
    "--db-url", "sqlite:///data/ironforge.db?mode=rwc"]

# Labels
LABEL org.opencontainers.image.title="IronForge" \
      org.opencontainers.image.description="Rust Git Hosting Platform" \
      org.opencontainers.image.authors="lengyuqu" \
      org.opencontainers.image.source="https://github.com/lengyuqu/ironforge"
