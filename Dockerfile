# syntax=docker/dockerfile:1

# ---------- Build the web apps ----------
# Built once with no configuration: the server writes each page's configuration
# into it when serving, so one image runs anywhere.
FROM node:24-slim AS web-build

WORKDIR /app/web

COPY web/package.json web/pnpm-lock.yaml web/pnpm-workspace.yaml ./
RUN npm install -g "$(node -p 'require("./package.json").packageManager')"
RUN pnpm install --frozen-lockfile

COPY web ./
RUN pnpm run build

# ---------- Build the binary ----------
# cargo-chef caches the compiled dependency graph in its own layer, so a
# source-only change recompiles just our crates.
# https://github.com/LukeMathWalker/cargo-chef
FROM rust:1.94-bookworm AS chef
WORKDIR /app
RUN cargo install --locked cargo-chef

FROM chef AS planner
COPY Cargo.toml Cargo.lock ./
COPY crates ./crates
RUN cargo chef prepare --recipe-path recipe.json

# The web apps are embedded in the binary, so they are in place before it builds.
FROM chef AS build
COPY --from=planner /app/recipe.json recipe.json
RUN cargo chef cook --release --recipe-path recipe.json --bin caravel
COPY Cargo.toml Cargo.lock ./
COPY crates ./crates
COPY --from=web-build /app/web/dist ./web/dist
RUN cargo build --release --bin caravel

# ---------- Runtime ----------
FROM debian:bookworm-slim

RUN apt-get update \
  && apt-get install -y --no-install-recommends ca-certificates \
  && rm -rf /var/lib/apt/lists/* \
  && useradd --system --create-home --home-dir /app caravel

WORKDIR /app
COPY --from=build /app/target/release/caravel /app/caravel

USER caravel

ENV PORT=2892 \
    MEDIA_DIR=/app/data/media

EXPOSE 2892

CMD ["/app/caravel"]
