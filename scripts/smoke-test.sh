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

# Do not probe /api/healthz through the WSL TCP proxy. Jupyter MCP Server
# deliberately restricts management routes to requests whose *actual TCP peer*
# is localhost. The socket proxy preserves a local Host header but the backend
# sees the Docker bridge peer, so the management route correctly returns 421.
# Container health is already authoritative for the management endpoint; the
# public proxy path is validated below against the actual /mcp transport.
printf '[test] MCP container health... '
mcp_container="$(docker compose ps -q mcp)"
if [[ -z "${mcp_container}" ]]; then
  echo "MCP container is not running"
  exit 1
fi
if [[ "$(docker inspect --format '{{.State.Health.Status}}' "${mcp_container}")" != "healthy" ]]; then
  echo "MCP container is not healthy"
  docker compose logs --tail=100 mcp || true
  exit 1
fi
echo OK

printf '[test] MCP proxy rejects missing bearer token... '
status="$(curl -sS -o /dev/null -w '%{http_code}' \
  -H 'Content-Type: application/json' \
  -H 'Accept: application/json, text/event-stream' \
  -d '{}' \
  "http://127.0.0.1:${MCP_PORT}/mcp" || true)"
if [[ "${status}" == "401" || "${status}" == "403" ]]; then
  echo OK
else
  echo "expected 401/403 through MCP loopback proxy, got HTTP ${status}"
  docker compose logs --tail=100 mcp || true
  exit 1
fi

printf '[test] MCP proxy accepts bearer token before protocol validation... '
status="$(curl -sS -o /dev/null -w '%{http_code}' \
  -H "Authorization: Bearer ${MCP_TOKEN}" \
  -H 'Content-Type: application/json' \
  -H 'Accept: application/json, text/event-stream' \
  -d '{}' \
  "http://127.0.0.1:${MCP_PORT}/mcp" || true)"
if [[ "${status}" == "401" || "${status}" == "403" || "${status}" == "421" || "${status}" == "000" ]]; then
  echo "authentication, host validation, or connectivity failed through MCP loopback proxy with HTTP ${status}"
  docker compose logs --tail=100 mcp || true
  exit 1
fi
echo "OK (HTTP ${status}; malformed MCP body is intentional)"

printf '[test] CoKernel MCP extension installed... '
docker compose exec -T mcp python - <<'PY' >/dev/null
from importlib.metadata import entry_points
points = entry_points(group="jupyter_mcp_server.extensions")
assert any(ep.name == "zz-cokernel-session-attach" for ep in points), points
PY
echo OK

echo "CoKernel smoke test passed."
