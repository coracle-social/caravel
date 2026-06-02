# syntax=docker/dockerfile:1

# ---------- Build the Rust backend (compiled here, in the build stage) ----------
FROM rust:1.94-bookworm AS backend-build

WORKDIR /app

RUN apt-get update \
  && apt-get install -y --no-install-recommends \
    pkg-config \
    libsqlite3-dev \
  && rm -rf /var/lib/apt/lists/*

COPY backend/Cargo.toml backend/Cargo.lock ./
COPY backend/src ./src
COPY backend/migrations ./migrations

RUN cargo build --release

# ---------- Runtime: prebuilt backend + frontend built at startup ----------
# The frontend is built in this (run) stage, not at image-build time, so VITE_*
# values provided when the container starts are inlined into the bundle.
# node:20-slim is bookworm-based, so it is ABI-compatible with the backend binary.
FROM node:20-slim

RUN apt-get update \
  && apt-get install -y --no-install-recommends \
    ca-certificates \
    libsqlite3-0 \
  && rm -rf /var/lib/apt/lists/* \
  && npm install -g bun serve

WORKDIR /app

# Install frontend deps as a cached image layer. The actual `vite build` runs at
# startup (see the entrypoint below) so it can pick up runtime env vars.
COPY frontend/package.json frontend/bun.lock ./frontend/
RUN cd frontend && bun install

COPY frontend ./frontend

# Prebuilt backend binary from the build stage.
COPY --from=backend-build /app/target/release/backend /app/backend

# Single entrypoint: build the frontend with the runtime config, then run both
# processes and exit (so the orchestrator restarts us) if either one dies.
RUN cat > /app/entrypoint.sh <<'EOF'
#!/usr/bin/env bash
set -euo pipefail

echo "Building frontend with runtime configuration..."
(cd /app/frontend && bun run build)

echo "Starting backend (:2892) and frontend (:3000)..."
/app/backend &
backend_pid=$!
serve -s /app/frontend/dist -l 3000 &
serve_pid=$!

trap 'kill -TERM "$backend_pid" "$serve_pid" 2>/dev/null || true' TERM INT

# Exit as soon as either process exits.
wait -n
EOF
RUN chmod +x /app/entrypoint.sh

EXPOSE 2892 3000

CMD ["/app/entrypoint.sh"]
