#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "${ROOT_DIR}"

if [[ ! -f .env ]]; then
  "${ROOT_DIR}/scripts/init-env.sh" >/dev/null
fi

IFS= read -r tunnel_id || true
IFS= read -r api_key || true

if [[ "${tunnel_id}" == *$'\n'* || "${tunnel_id}" == *$'\r'* || "${api_key}" == *$'\n'* || "${api_key}" == *$'\r'* ]]; then
  echo "Tunnel credentials must be single-line values." >&2
  exit 2
fi

python3 - "$tunnel_id" "$api_key" <<'PY'
from pathlib import Path
import sys

path = Path('.env')
values = {
    'CONTROL_PLANE_TUNNEL_ID': sys.argv[1],
    'CONTROL_PLANE_API_KEY': sys.argv[2],
}
lines = path.read_text(encoding='utf-8').splitlines()
seen = set()
out = []
for line in lines:
    if '=' in line and not line.lstrip().startswith('#'):
        key = line.split('=', 1)[0]
        if key in values:
            out.append(f'{key}={values[key]}')
            seen.add(key)
            continue
    out.append(line)
for key, value in values.items():
    if key not in seen:
        out.append(f'{key}={value}')
path.write_text('\n'.join(out) + '\n', encoding='utf-8')
PY

chmod 600 .env 2>/dev/null || true
echo "CoKernel tunnel configuration updated."
