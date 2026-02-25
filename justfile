dev:
    #!/usr/bin/env sh
    trap 'kill 0' EXIT
    cd backend && cargo run &
    cd frontend && bun dev &
    wait
