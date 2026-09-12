#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "${ROOT_DIR}"

if [[ ! -f .env ]]; then
  echo "Missing .env. Run ./scripts/init-env.sh first."
  exit 1
fi

get_env() {
  local key="$1"
  awk -F= -v key="${key}" '$1 == key {sub(/^[^=]*=/, ""); gsub(/\r/, ""); print; exit}' .env
}

JUPYTER_TOKEN="$(get_env JUPYTER_TOKEN)"
JUPYTER_PORT="$(get_env JUPYTER_PORT)"
JUPYTER_PORT="${JUPYTER_PORT:-8888}"
MCP_TOKEN="$(get_env MCP_TOKEN)"
MCP_PORT="$(get_env MCP_PORT)"
MCP_PORT="${MCP_PORT:-4040}"

printf '[test] Jupyter service... '
docker compose exec -T jupyter python -c 'import jupyterlab; print(jupyterlab.__version__)' >/dev/null
echo OK

printf '[test] uv workspace kernel... '
docker compose exec -T jupyter sh -lc 'test -x /workspace/.venv/bin/python && /workspace/.venv/bin/python -c "import ipykernel"'
echo OK

printf '[test] GPU device exposed to workbench... '
docker compose exec -T jupyter sh -lc 'test -e /dev/dxg || command -v nvidia-smi >/dev/null 2>&1'
echo OK

printf '[test] WSL loopback proxy sockets... '
systemctl is-active --quiet cokernel-jupyter-proxy.socket
systemctl is-active --quiet cokernel-mcp-proxy.socket
echo OK

printf '[test] Jupyter through WSL loopback proxy... '
curl -fsS --max-time 5 "http://127.0.0.1:${JUPYTER_PORT}/api?token=${JUPYTER_TOKEN}" >/dev/null
echo OK

printf '[test] MCP health through WSL loopback proxy... '
curl -fsS --max-time 5 "http://127.0.0.1:${MCP_PORT}/api/healthz" >/dev/null
echo OK

printf '[test] CoKernel MCP extension installed... '
docker compose exec -T mcp python - <<'PY' >/dev/null
from importlib.metadata import entry_points
points = entry_points(group="jupyter_mcp_server.extensions")
assert any(ep.name == "zz-cokernel-session-attach" for ep in points), points
PY
echo OK

printf '[test] MCP rejects missing bearer token... '
status="$(curl -sS -o /dev/null -w '%{http_code}' \
  -H 'Content-Type: application/json' \
  -H 'Accept: application/json, text/event-stream' \
  -d '{}' \
  "http://127.0.0.1:${MCP_PORT}/mcp" || true)"
if [[ "${status}" == "401" || "${status}" == "403" ]]; then
  echo OK
else
  echo "expected 401/403, got HTTP ${status}"
  exit 1
fi

printf '[test] MCP accepts bearer token before protocol validation... '
status="$(curl -sS -o /dev/null -w '%{http_code}' \
  -H "Authorization: Bearer ${MCP_TOKEN}" \
  -H 'Content-Type: application/json' \
  -H 'Accept: application/json, text/event-stream' \
  -d '{}' \
  "http://127.0.0.1:${MCP_PORT}/mcp" || true)"
if [[ "${status}" == "401" || "${status}" == "403" || "${status}" == "000" ]]; then
  echo "authentication or connectivity failed with HTTP ${status}"
  exit 1
fi
echo "OK (HTTP ${status}; malformed MCP body is intentional)"

echo "CoKernel smoke test passed."
