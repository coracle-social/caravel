dev:
  #!/usr/bin/env sh
  trap 'kill 0' EXIT
  cd backend && onchange src -ik -- bash -c 'RUST_LOG=backend=info cargo run' &
  cd frontend && bun dev &
  wait

fmt-backend:
  cd backend && cargo fmt

fmt: fmt-backend

lint-backend:
  cd backend && cargo clippy -- -D warnings

lint: lint-backend

build-backend:
  cd backend && cargo build

build: build-backend

check: fmt lint build
