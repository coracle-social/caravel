dev:
  #!/usr/bin/env sh
  trap 'kill 0' EXIT
  cd backend && onchange src -ik -- bash -c 'RUST_LOG=backend=info cargo run' &
  cd frontend && bun dev &
  wait

dev-frontend:
  cd frontend && bun run dev

build-frontend:
  cd frontend && bun run build

preview-frontend:
  cd frontend && bun run preview

fmt-backend:
  cd backend && cargo fmt

fmt: fmt-backend

lint-backend:
  cd backend && cargo clippy -- -D warnings

lint: lint-backend

build-backend:
  cd backend && cargo build

build: build-backend build-frontend

check: fmt lint build
