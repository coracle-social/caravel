#!/usr/bin/env bash
# Substitute the real config into the prebuilt frontend bundle, then run the
# backend and static server, exiting (so the orchestrator restarts us) if either
# process dies.
set -euo pipefail

# Map the provided runtime variables onto the frontend's VITE_* placeholders.
VITE_API_URL="${SERVER_URL:-}"
VITE_RELAY_DOMAIN="${RELAY_DOMAIN:-}"
VITE_PLATFORM_NAME="${PLATFORM_NAME:-}"
VITE_PLATFORM_LOGO="${PLATFORM_LOGO:-/caravel.png}"

# Escape characters that are special in a sed replacement.
esc() { printf '%s' "$1" | sed -e 's/[&|\\]/\\&/g'; }

echo "Applying runtime configuration to the frontend bundle..."
while IFS= read -r -d '' f; do
  sed -i \
    -e "s|__VITE_API_URL__|$(esc "$VITE_API_URL")|g" \
    -e "s|__VITE_RELAY_DOMAIN__|$(esc "$VITE_RELAY_DOMAIN")|g" \
    -e "s|__VITE_PLATFORM_NAME__|$(esc "$VITE_PLATFORM_NAME")|g" \
    -e "s|__VITE_PLATFORM_LOGO__|$(esc "$VITE_PLATFORM_LOGO")|g" \
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
