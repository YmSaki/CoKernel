#!/usr/bin/env bash
set -euo pipefail

: "${JUPYTER_TOKEN:?JUPYTER_TOKEN is required}"

mkdir -p /workspace/notebooks /workspace/data

if [[ ! -f /workspace/pyproject.toml ]]; then
  cat > /workspace/pyproject.toml <<'EOF'
[project]
name = "cokernel-workspace"
version = "0.1.0"
description = "Local CoKernel notebook workspace"
requires-python = ">=3.12"
dependencies = [
  "ipykernel>=6.30,<7",
]
EOF
fi

if [[ "${COKERNEL_SYNC_WORKSPACE:-1}" == "1" ]]; then
  echo "[cokernel] syncing /workspace with uv"
  uv sync --project /workspace
fi

if [[ -x /workspace/.venv/bin/python ]]; then
  /workspace/.venv/bin/python -m ipykernel install \
    --user \
    --name cokernel-workspace \
    --display-name "CoKernel workspace" >/dev/null
fi

echo "[cokernel] starting JupyterLab on :8888"
exec jupyter lab \
  --ServerApp.root_dir=/workspace \
  --ServerApp.ip=0.0.0.0 \
  --ServerApp.port=8888 \
  --ServerApp.open_browser=False \
  --IdentityProvider.token="${JUPYTER_TOKEN}"
