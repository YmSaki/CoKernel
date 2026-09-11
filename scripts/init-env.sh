#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
ENV_FILE="${ROOT_DIR}/.env"
EXAMPLE_FILE="${ROOT_DIR}/.env.example"

cd "${ROOT_DIR}"

if [[ ! -f "${ENV_FILE}" ]]; then
  cp "${EXAMPLE_FILE}" "${ENV_FILE}"
fi

chmod 600 "${ENV_FILE}"

generate_token() {
  if command -v openssl >/dev/null 2>&1; then
    openssl rand -hex 32
  else
    head -c 32 /dev/urandom | od -An -tx1 | tr -d ' \n'
  fi
}

set_if_empty() {
  local key="$1"
  local value="$2"
  local current

  current="$(awk -F= -v key="${key}" '$1 == key {sub(/^[^=]*=/, ""); print; exit}' "${ENV_FILE}")"
  if [[ -n "${current}" ]]; then
    return
  fi

  if grep -q "^${key}=" "${ENV_FILE}"; then
    local tmp
    tmp="$(mktemp)"
    awk -v key="${key}" -v value="${value}" '
      index($0, key "=") == 1 { print key "=" value; next }
      { print }
    ' "${ENV_FILE}" > "${tmp}"
    cat "${tmp}" > "${ENV_FILE}"
    rm -f "${tmp}"
  else
    printf '%s=%s\n' "${key}" "${value}" >> "${ENV_FILE}"
  fi
}

set_if_empty JUPYTER_TOKEN "$(generate_token)"
set_if_empty MCP_TOKEN "$(generate_token)"

mkdir -p "${ROOT_DIR}/workspace"
touch "${ROOT_DIR}/workspace/.gitkeep"

echo "CoKernel environment initialized: ${ENV_FILE}"
echo "Local Jupyter/MCP tokens are present."
echo "Add CONTROL_PLANE_TUNNEL_ID and CONTROL_PLANE_API_KEY to enable ChatGPT tunnel access."
