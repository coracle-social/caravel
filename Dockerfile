# syntax=docker/dockerfile:1

# ---------- Build the Rust backend ----------
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

# ---------- Build the frontend with placeholder config ----------
# The frontend is compiled once, here, with sentinel VITE_* values. The runtime
# entrypoint find-and-replaces those sentinels with the real configuration when
# the container starts, so one image can be deployed with any config — no rebuild
# and no build step at startup.
FROM node:20-slim AS frontend-build

RUN npm install -g bun

WORKDIR /app/frontend

COPY frontend/package.json frontend/bun.lock ./
RUN bun install

COPY frontend ./

ENV VITE_API_URL=__VITE_API_URL__ \
    VITE_RELAY_DOMAIN=__VITE_RELAY_DOMAIN__ \
    VITE_PLATFORM_NAME=__VITE_PLATFORM_NAME__
RUN bun run build

# ---------- Runtime ----------
# node:20-slim is bookworm-based, so it is ABI-compatible with the backend binary.
# No bun / frontend sources here — just the prebuilt bundle and a static server.
FROM node:20-slim

RUN apt-get update \
  && apt-get install -y --no-install-recommends \
    ca-certificates \
    libsqlite3-0 \
  && rm -rf /var/lib/apt/lists/* \
  && npm install -g serve

WORKDIR /app

COPY --from=backend-build /app/target/release/backend /app/backend
COPY --from=frontend-build /app/frontend/dist /app/dist

# Single entrypoint: substitute the real config into the prebuilt bundle, then
# run both processes and exit (so the orchestrator restarts us) if either dies.
RUN cat > /app/entrypoint.sh <<'EOF'
#!/usr/bin/env bash
set -euo pipefail

# Map the provided runtime variables onto the frontend's VITE_* placeholders.
VITE_API_URL="${SERVER_URL:-}"
VITE_RELAY_DOMAIN="${RELAY_DOMAIN:-}"
VITE_PLATFORM_NAME="${PLATFORM_NAME:-}"

# Escape characters that are special in a sed replacement.
esc() { printf '%s' "$1" | sed -e 's/[&|\\]/\\&/g'; }

echo "Applying runtime configuration to the frontend bundle..."
while IFS= read -r -d '' f; do
  sed -i \
    -e "s|__VITE_API_URL__|$(esc "$VITE_API_URL")|g" \
    -e "s|__VITE_RELAY_DOMAIN__|$(esc "$VITE_RELAY_DOMAIN")|g" \
    -e "s|__VITE_PLATFORM_NAME__|$(esc "$VITE_PLATFORM_NAME")|g" \
    "$f"
done < <(find /app/dist -type f \( -name '*.js' -o -name '*.html' \) -print0)

echo "Starting backend (:2892) and frontend (:3000)..."
/app/backend &
backend_pid=$!
serve -s /app/dist -l 3000 &
serve_pid=$!

trap 'kill -TERM "$backend_pid" "$serve_pid" 2>/dev/null || true' TERM INT

# Exit as soon as either process exits.
wait -n
EOF
RUN chmod +x /app/entrypoint.sh

EXPOSE 2892 3000

CMD ["/app/entrypoint.sh"]
