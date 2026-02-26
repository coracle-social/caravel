dev:
    #!/usr/bin/env sh
    trap 'kill 0' EXIT
    cd backend && RUST_LOG=backend=info cargo run &
    cd frontend && bun dev &
    wait
