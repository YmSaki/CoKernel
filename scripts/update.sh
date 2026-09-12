#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "${ROOT_DIR}"

echo "[update] normalizing local environment"
"${ROOT_DIR}/scripts/init-env.sh"

echo "[update] stopping existing CoKernel containers before network migration"
docker compose --profile tunnel down --remove-orphans || docker compose down --remove-orphans || true

echo "[update] configuring Windows/WSL loopback bridge"
"${ROOT_DIR}/scripts/configure-wsl-loopback-proxy.sh"

echo "[update] validating WSL, Docker, and GPU"
"${ROOT_DIR}/scripts/doctor.sh"

echo "[update] rebuilding and starting CoKernel"
"${ROOT_DIR}/up.sh"

echo "[update] running service smoke tests"
"${ROOT_DIR}/scripts/smoke-test.sh"

echo "CoKernel WSL update completed successfully."
