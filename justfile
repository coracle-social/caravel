# Start the local Postgres the app and tests use.
pg:
  podman start caravel-pg 2>/dev/null || podman run -d --name caravel-pg -e POSTGRES_USER=caravel -e POSTGRES_PASSWORD=caravel -e POSTGRES_DB=caravel -p 127.0.0.1:55433:5432 docker.io/library/postgres:17-alpine

# Recreate the local database, which a change to the squashed schema requires.
reset-db:
  podman exec caravel-pg psql -U caravel -d postgres -c 'DROP DATABASE IF EXISTS caravel WITH (FORCE)' -c 'CREATE DATABASE caravel'

dev:
  #!/usr/bin/env sh
  trap 'kill 0' EXIT
  just dev-backend &
  just dev-web &
  wait

dev-backend:
  onchange crates -ik -- bash -c 'RUST_LOG=caravel=info,caravel_core=info,caravel_api=info cargo run -p caravel-app -- serve'

dev-web:
  cd web && pnpm run dev

fmt-backend:
  cargo fmt --all

lint-backend:
  cargo clippy --workspace --all-targets -- -D warnings

test-backend:
  cargo test --workspace

build-backend:
  cargo build --workspace

build-web:
  cd web && pnpm install && pnpm run build

fmt: fmt-backend

lint: lint-backend

build: build-backend build-web

test: test-backend

check: fmt lint build test
