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
COPY --chmod=0755 entrypoint.sh /app/entrypoint.sh

RUN mkdir -p /app/data && chown -R node:node /app/dist /app/data

USER node:node

ENV SERVER_PORT=2892 \
    DATABASE_URL=sqlite:///app/data/caravel.db

EXPOSE 2892 3000

CMD ["/app/entrypoint.sh"]
