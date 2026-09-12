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
else
  echo "[cokernel] starting Jupyter + MCP (tunnel credentials are not configured)"
  docker compose up -d --build --wait --wait-timeout 180
  TUNNEL_STATE="disabled"
fi

echo
echo "CoKernel is healthy."
echo "  JupyterLab: http://localhost:${JUPYTER_PORT}"
echo "  Tunnel:     ${TUNNEL_STATE}"
if [[ "${TUNNEL_STATE}" == "enabled" ]]; then
  echo "  Tunnel UI:  http://localhost:${TUNNEL_HEALTH_PORT}/ui"
fi
echo
echo "Use ./logs.sh if a service later becomes unhealthy."
