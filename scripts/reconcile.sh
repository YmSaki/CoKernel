#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "${ROOT_DIR}"

"${ROOT_DIR}/scripts/init-env.sh" >/dev/null
"${ROOT_DIR}/scripts/configure-wsl-loopback-proxy.sh" >/dev/null

get_env() {
  local key="$1"
  awk -F= -v key="${key}" '$1 == key {sub(/^[^=]*=/, ""); gsub(/\r/, ""); print; exit}' .env
}

TUNNEL_ID="$(get_env CONTROL_PLANE_TUNNEL_ID)"
TUNNEL_KEY="$(get_env CONTROL_PLANE_API_KEY)"
TUNNEL_HEALTH_PORT="$(get_env TUNNEL_HEALTH_PORT)"
TUNNEL_HEALTH_PORT="${TUNNEL_HEALTH_PORT:-8080}"

if ! docker info >/dev/null 2>&1; then
  echo "Docker is not reachable." >&2
  exit 1
fi

# Reconcile the currently installed runtime without rebuilding images. This is
# safe for automatic recovery because healthy containers are left in place.
docker compose up -d --wait --wait-timeout 120 jupyter mcp

if [[ -n "${TUNNEL_ID}" && -n "${TUNNEL_KEY}" ]]; then
  docker compose --profile tunnel up -d --no-deps tunnel
  deadline=$((SECONDS + 60))
  until curl -fsS --max-time 3 "http://127.0.0.1:${TUNNEL_HEALTH_PORT}/readyz" >/dev/null 2>&1; do
    if (( SECONDS >= deadline )); then
      echo "Tunnel did not become ready during reconcile." >&2
      docker compose --profile tunnel ps >&2 || true
      docker compose --profile tunnel logs --tail=80 tunnel >&2 || true
      exit 1
    fi
    sleep 2
  done
fi

echo "CoKernel runtime reconciled."
