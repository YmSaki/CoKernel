#!/usr/bin/env bash
set -uo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "${ROOT_DIR}" || exit 1

failures=0
warnings=0

ok() { printf '[OK]   %s\n' "$*"; }
warn() { printf '[WARN] %s\n' "$*"; warnings=$((warnings + 1)); }
fail() { printf '[FAIL] %s\n' "$*"; failures=$((failures + 1)); }

if grep -qi microsoft /proc/sys/kernel/osrelease 2>/dev/null; then
  ok "running under WSL"
else
  warn "not running under WSL; CoKernel is optimized for WSL2 but can run on Linux"
fi

if [[ "$(cat /proc/sys/kernel/osrelease 2>/dev/null)" == *WSL2* ]] || grep -qi microsoft-standard /proc/sys/kernel/osrelease 2>/dev/null; then
  ok "WSL2 kernel detected"
fi

if [[ -d /mnt/c ]]; then
  warn "/mnt/c is mounted; recommended dedicated-WSL isolation is not active"
else
  ok "Windows C: is not auto-mounted"
fi

if command -v cmd.exe >/dev/null 2>&1 || command -v powershell.exe >/dev/null 2>&1; then
  warn "Windows interop is available; run ./scripts/harden-wsl.sh in a dedicated distro"
else
  ok "Windows executable interop is unavailable"
fi

if command -v docker >/dev/null 2>&1; then
  ok "docker CLI found"
else
  fail "docker CLI not found"
fi

if docker info >/dev/null 2>&1; then
  ok "Docker daemon reachable"
else
  fail "Docker daemon is not reachable by the current user"
fi

if docker compose version >/dev/null 2>&1; then
  ok "Docker Compose plugin found"
else
  fail "Docker Compose plugin not found"
fi

if command -v nvidia-smi >/dev/null 2>&1; then
  if nvidia-smi >/dev/null 2>&1; then
    ok "NVIDIA GPU visible from WSL"
  else
    fail "nvidia-smi exists but failed"
  fi
elif [[ -x /usr/lib/wsl/lib/nvidia-smi ]]; then
  if /usr/lib/wsl/lib/nvidia-smi >/dev/null 2>&1; then
    ok "NVIDIA GPU visible from WSL (/usr/lib/wsl/lib/nvidia-smi)"
  else
    fail "WSL NVIDIA SMI exists but failed"
  fi
else
  fail "NVIDIA GPU is not visible in WSL; verify the Windows NVIDIA driver and WSL GPU support"
fi

if [[ -f .env ]]; then
  ok ".env exists"
else
  warn ".env does not exist; run ./scripts/init-env.sh"
fi

if docker info >/dev/null 2>&1; then
  gpu_test_image="nvidia/cuda:12.8.1-base-ubuntu24.04"
  if [[ -f .env ]]; then
    configured="$(awk -F= '$1 == "GPU_TEST_IMAGE" {sub(/^[^=]*=/, ""); print; exit}' .env)"
    gpu_test_image="${configured:-${gpu_test_image}}"
  fi

  echo "[INFO] testing GPU access in Docker (${gpu_test_image})"
  if docker run --rm --gpus all "${gpu_test_image}" nvidia-smi >/dev/null 2>&1; then
    ok "GPU visible inside Docker container"
  else
    fail "GPU test container failed; check NVIDIA Container Toolkit configuration"
  fi
fi

if [[ "${failures}" -gt 0 ]]; then
  echo
  echo "Doctor result: ${failures} failure(s), ${warnings} warning(s)."
  exit 1
fi

echo
 echo "Doctor result: healthy (${warnings} warning(s))."
