#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "${ROOT_DIR}"

"${ROOT_DIR}/scripts/init-env.sh" >/dev/null

get_env() {
  local key="$1"
  awk -F= -v key="${key}" '$1 == key {sub(/^[^=]*=/, ""); gsub(/\r/, ""); print; exit}' .env
}

TUNNEL_ID="$(get_env CONTROL_PLANE_TUNNEL_ID)"
TUNNEL_KEY="$(get_env CONTROL_PLANE_API_KEY)"
JUPYTER_PORT="$(get_env JUPYTER_PORT)"
JUPYTER_PORT="${JUPYTER_PORT:-8888}"
TUNNEL_HEALTH_PORT="$(get_env TUNNEL_HEALTH_PORT)"
TUNNEL_HEALTH_PORT="${TUNNEL_HEALTH_PORT:-8080}"

if ! docker info >/dev/null 2>&1; then
  echo "Docker is not reachable as the current user."
  echo "Run ./scripts/bootstrap-ubuntu.sh, then restart the WSL distribution (or refresh docker group membership)."
  exit 1
fi

# Docker can publish a port to WSL's 127.0.0.1 through NAT rules without
# creating a userspace listening socket. WSL localhostForwarding mirrors real
# listening sockets to Windows, so keep a systemd socket-proxy in front of the
# private Docker backend ports. This is idempotent and also applies migrations
# when an existing installation is started after an update.
"${ROOT_DIR}/scripts/configure-wsl-loopback-proxy.sh"

if [[ -n "${TUNNEL_ID}" && -n "${TUNNEL_KEY}" ]]; then
  echo "[cokernel] starting Jupyter + MCP + Secure MCP Tunnel"
  docker compose --profile tunnel up -d --build --wait --wait-timeout 180
  TUNNEL_STATE="enabled"

  # Docker Compose can only wait for the tunnel container to be running because
  # tunnel-client does not expose a container healthcheck here. /readyz is the
  # authoritative signal that the runtime is actually ready to accept tunneled
  # connector traffic. Probe it through the same WSL loopback bridge Windows
  # uses so startup also validates that path.
  tunnel_ready_url="http://127.0.0.1:${TUNNEL_HEALTH_PORT}/readyz"
  echo "[cokernel] waiting for Secure MCP Tunnel readiness"
  deadline=$((SECONDS + 60))
  until curl -fsS --max-time 3 "${tunnel_ready_url}" >/dev/null 2>&1; do
    if (( SECONDS >= deadline )); then
      echo "Secure MCP Tunnel did not become ready within 60 seconds."
      echo "Readiness endpoint: ${tunnel_ready_url}"
      docker compose --profile tunnel logs --tail=100 tunnel || true
      exit 1
    fi
    sleep 2
  done
  TUNNEL_STATE="enabled (ready)"
else
  echo "[cokernel] starting Jupyter + MCP (tunnel credentials are not configured)"
  docker compose up -d --build --wait --wait-timeout 180
  TUNNEL_STATE="disabled"
fi

echo
echo "CoKernel is healthy."
echo "  JupyterLab: http://localhost:${JUPYTER_PORT}"
echo "  Tunnel:     ${TUNNEL_STATE}"
if [[ "${TUNNEL_STATE}" == "enabled (ready)" ]]; then
  echo "  Tunnel readiness: http://localhost:${TUNNEL_HEALTH_PORT}/readyz"
  echo "  Tunnel admin UI:  intentionally not exposed through the WSL proxy"
fi
echo
echo "Use ./logs.sh if a service later becomes unhealthy."
