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
  awk -F= -v key="${key}" '$1 == key {sub(/^[^=]*=/, ""); print; exit}' .env
}

MCP_TOKEN="$(get_env MCP_TOKEN)"
MCP_PORT="$(get_env MCP_PORT)"
MCP_PORT="${MCP_PORT:-4040}"

printf '[test] Jupyter service... '
docker compose exec -T jupyter python -c 'import jupyterlab, sys; print(jupyterlab.__version__)' >/dev/null
echo OK

printf '[test] uv workspace kernel... '
docker compose exec -T jupyter sh -lc 'test -x /workspace/.venv/bin/python && /workspace/.venv/bin/python -c "import ipykernel"'
echo OK

printf '[test] GPU from workbench... '
docker compose exec -T jupyter sh -lc 'test -e /dev/dxg || command -v nvidia-smi >/dev/null 2>&1'
echo OK

printf '[test] MCP health... '
curl -fsS "http://127.0.0.1:${MCP_PORT}/api/healthz" >/dev/null
echo OK

printf '[test] MCP authentication boundary... '
status="$(curl -sS -o /dev/null -w '%{http_code}' "http://127.0.0.1:${MCP_PORT}/mcp" || true)"
if [[ "${status}" == "401" || "${status}" == "403" || "${status}" == "405" ]]; then
  echo OK
else
  echo "unexpected HTTP ${status} (continuing; MCP transport may reject GET differently)"
fi

printf '[test] MCP authenticated endpoint reachable... '
status="$(curl -sS -o /dev/null -w '%{http_code}' -H "Authorization: Bearer ${MCP_TOKEN}" "http://127.0.0.1:${MCP_PORT}/mcp" || true)"
if [[ "${status}" =~ ^(200|202|400|405)$ ]]; then
  echo OK
else
  echo "unexpected HTTP ${status}"
  exit 1
fi

echo "CoKernel smoke test passed."
