#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
ENV_FILE="${ROOT_DIR}/.env"

if [[ ! -f "${ENV_FILE}" ]]; then
  echo "Missing ${ENV_FILE}. Run ./scripts/init-env.sh first."
  exit 1
fi

if ! command -v systemctl >/dev/null 2>&1 || [[ "$(ps -p 1 -o comm= 2>/dev/null)" != "systemd" ]]; then
  echo "systemd must be active before configuring the CoKernel Windows/WSL loopback bridge."
  exit 1
fi

PROXY_BIN="$(command -v systemd-socket-proxyd 2>/dev/null || true)"
if [[ -z "${PROXY_BIN}" ]]; then
  for candidate in /usr/lib/systemd/systemd-socket-proxyd /lib/systemd/systemd-socket-proxyd; do
    if [[ -x "${candidate}" ]]; then
      PROXY_BIN="${candidate}"
      break
    fi
  done
fi
if [[ -z "${PROXY_BIN}" ]]; then
  echo "systemd-socket-proxyd was not found. The Ubuntu systemd package is incomplete or unsupported."
  exit 1
fi

get_env() {
  local key="$1"
  local fallback="$2"
  local value
  value="$(awk -F= -v key="${key}" '$1 == key {sub(/^[^=]*=/, ""); gsub(/\r/, ""); print; exit}' "${ENV_FILE}")"
  printf '%s' "${value:-${fallback}}"
}

validate_port() {
  local name="$1"
  local value="$2"
  if [[ ! "${value}" =~ ^[0-9]+$ ]] || (( value < 1 || value > 65535 )); then
    echo "${name} must be an integer from 1 to 65535; got '${value}'."
    exit 1
  fi
}

port_is_listening() {
  local port="$1"
  ss -ltnH 2>/dev/null | awk '{print $4}' | grep -Eq "(^|:)$port$"
}

JUPYTER_PORT="$(get_env JUPYTER_PORT 8888)"
MCP_PORT="$(get_env MCP_PORT 4040)"
TUNNEL_HEALTH_PORT="$(get_env TUNNEL_HEALTH_PORT 8080)"
JUPYTER_BACKEND_PORT="$(get_env JUPYTER_BACKEND_PORT 18888)"
MCP_BACKEND_PORT="$(get_env MCP_BACKEND_PORT 14040)"
TUNNEL_HEALTH_BACKEND_PORT="$(get_env TUNNEL_HEALTH_BACKEND_PORT 18080)"

for pair in \
  "JUPYTER_PORT:${JUPYTER_PORT}" \
  "MCP_PORT:${MCP_PORT}" \
  "TUNNEL_HEALTH_PORT:${TUNNEL_HEALTH_PORT}" \
  "JUPYTER_BACKEND_PORT:${JUPYTER_BACKEND_PORT}" \
  "MCP_BACKEND_PORT:${MCP_BACKEND_PORT}" \
  "TUNNEL_HEALTH_BACKEND_PORT:${TUNNEL_HEALTH_BACKEND_PORT}"; do
  validate_port "${pair%%:*}" "${pair#*:}"
done

if [[ "${JUPYTER_PORT}" == "${JUPYTER_BACKEND_PORT}" || \
      "${MCP_PORT}" == "${MCP_BACKEND_PORT}" || \
      "${TUNNEL_HEALTH_PORT}" == "${TUNNEL_HEALTH_BACKEND_PORT}" ]]; then
  echo "Each public port must differ from its private Docker backend port."
  exit 1
fi

if [[ "${JUPYTER_PORT}" == "${MCP_PORT}" || \
      "${JUPYTER_PORT}" == "${TUNNEL_HEALTH_PORT}" || \
      "${MCP_PORT}" == "${TUNNEL_HEALTH_PORT}" ]]; then
  echo "JUPYTER_PORT, MCP_PORT, and TUNNEL_HEALTH_PORT must be unique."
  exit 1
fi

# Stop any prior CoKernel socket units before checking port ownership. On the
# first migration from the old Compose model, Docker itself may still occupy
# the public ports. If so, stop only the existing CoKernel project once so the
# real systemd listeners can claim those ports. Future starts keep containers
# running because Docker uses the separate backend ports.
for unit in \
  cokernel-jupyter-proxy \
  cokernel-mcp-proxy \
  cokernel-tunnel-health-proxy; do
  sudo systemctl stop "${unit}.socket" "${unit}.service" >/dev/null 2>&1 || true
done

busy_public_port=false
for port in "${JUPYTER_PORT}" "${MCP_PORT}" "${TUNNEL_HEALTH_PORT}"; do
  if port_is_listening "${port}"; then
    busy_public_port=true
  fi
done

if [[ "${busy_public_port}" == "true" ]] && docker compose ps -q 2>/dev/null | grep -q .; then
  echo "[cokernel] migrating old Docker public-port bindings to private backend ports"
  docker compose --profile tunnel down --remove-orphans || docker compose down --remove-orphans
fi

for port in "${JUPYTER_PORT}" "${MCP_PORT}" "${TUNNEL_HEALTH_PORT}"; do
  if port_is_listening "${port}"; then
    echo "Public CoKernel port ${port} is already owned by another process."
    ss -ltnp 2>/dev/null | grep -E "(^|:)${port}[[:space:]]" || true
    exit 1
  fi
done

write_proxy_units() {
  local name="$1"
  local description="$2"
  local public_port="$3"
  local backend_port="$4"
  local socket_tmp service_tmp

  socket_tmp="$(mktemp)"
  service_tmp="$(mktemp)"

  cat >"${socket_tmp}" <<EOF
[Unit]
Description=${description} socket for Windows localhost forwarding

[Socket]
ListenStream=127.0.0.1:${public_port}
NoDelay=true

[Install]
WantedBy=sockets.target
EOF

  cat >"${service_tmp}" <<EOF
[Unit]
Description=${description} proxy to Docker backend
Requires=${name}.socket
After=${name}.socket network.target

[Service]
Type=notify
ExecStart=${PROXY_BIN} 127.0.0.1:${backend_port}
NoNewPrivileges=true
PrivateTmp=true
ProtectSystem=strict
ProtectHome=true
EOF

  sudo install -m 0644 "${socket_tmp}" "/etc/systemd/system/${name}.socket"
  sudo install -m 0644 "${service_tmp}" "/etc/systemd/system/${name}.service"
  rm -f "${socket_tmp}" "${service_tmp}"
}

write_proxy_units \
  cokernel-jupyter-proxy \
  "CoKernel Jupyter loopback" \
  "${JUPYTER_PORT}" \
  "${JUPYTER_BACKEND_PORT}"
write_proxy_units \
  cokernel-mcp-proxy \
  "CoKernel MCP loopback" \
  "${MCP_PORT}" \
  "${MCP_BACKEND_PORT}"
write_proxy_units \
  cokernel-tunnel-health-proxy \
  "CoKernel tunnel health loopback" \
  "${TUNNEL_HEALTH_PORT}" \
  "${TUNNEL_HEALTH_BACKEND_PORT}"

sudo systemctl daemon-reload
sudo systemctl enable --now \
  cokernel-jupyter-proxy.socket \
  cokernel-mcp-proxy.socket \
  cokernel-tunnel-health-proxy.socket >/dev/null

metadata_tmp="$(mktemp)"
cat >"${metadata_tmp}" <<EOF
JUPYTER_PORT=${JUPYTER_PORT}
MCP_PORT=${MCP_PORT}
TUNNEL_HEALTH_PORT=${TUNNEL_HEALTH_PORT}
JUPYTER_BACKEND_PORT=${JUPYTER_BACKEND_PORT}
MCP_BACKEND_PORT=${MCP_BACKEND_PORT}
TUNNEL_HEALTH_BACKEND_PORT=${TUNNEL_HEALTH_BACKEND_PORT}
EOF
sudo install -d -m 0755 /var/lib/cokernel
sudo install -m 0644 "${metadata_tmp}" /var/lib/cokernel/loopback-proxy.env
rm -f "${metadata_tmp}"

for unit in \
  cokernel-jupyter-proxy.socket \
  cokernel-mcp-proxy.socket \
  cokernel-tunnel-health-proxy.socket; do
  if ! systemctl is-active --quiet "${unit}"; then
    echo "Failed to activate ${unit}."
    sudo systemctl status --no-pager "${unit}" || true
    exit 1
  fi
done

echo "CoKernel WSL loopback bridge configured:"
echo "  Jupyter: Windows localhost:${JUPYTER_PORT} -> WSL proxy -> Docker localhost:${JUPYTER_BACKEND_PORT}"
echo "  MCP:     Windows localhost:${MCP_PORT} -> WSL proxy -> Docker localhost:${MCP_BACKEND_PORT}"
echo "  Tunnel:  Windows localhost:${TUNNEL_HEALTH_PORT} -> WSL proxy -> Docker localhost:${TUNNEL_HEALTH_BACKEND_PORT}"
