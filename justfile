dev:
  #!/usr/bin/env sh
  trap 'kill 0' EXIT
  cd backend && onchange src -ik -- bash -c 'RUST_LOG=backend=info cargo run' &
  cd frontend && bun dev &
  wait

dev-backend:
  cd backend && onchange src -ik -- bash -c 'RUST_LOG=backend=info cargo run'

dev-frontend:
  cd frontend && bun run dev

preview-frontend:
  cd frontend && bun run preview

fmt-backend:
  cd backend && cargo fmt

lint-backend:
  cd backend && cargo clippy --all-targets -- -D warnings

test-backend:
  cd backend && cargo test

test-backend-api:
  cd backend && cargo test api::tests::

build-backend:
  cd backend && cargo build

build-frontend:
  cd frontend && bun i && bun run build

fmt: fmt-backend

lint: lint-backend

build: build-backend build-frontend

test: test-backend

check: fmt lint build test
