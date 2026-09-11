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

if mountpoint -q /mnt/c 2>/dev/null; then
  warn "/mnt/c is mounted; recommended dedicated-WSL isolation is not active"
else
  ok "Windows C: is not mounted"
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

if docker info >/dev/null 2>&1; then
  runtimes="$(docker info --format '{{json .Runtimes}}' 2>/dev/null || true)"
  if [[ "${runtimes}" == *'"nvidia"'* ]]; then
    ok "NVIDIA runtime registered with Docker"
  else
    fail "NVIDIA runtime is not registered with Docker"
    echo "[INFO] Docker runtimes: ${runtimes:-unavailable}"
  fi
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
    configured="$(awk -F= '$1 == "GPU_TEST_IMAGE" {sub(/^[^=]*=/, ""); print; exit}' .env | tr -d '\r\n')"
    gpu_test_image="${configured:-${gpu_test_image}}"
  fi

  echo "[INFO] testing GPU access in Docker with explicit NVIDIA runtime (${gpu_test_image})"
  gpu_test_output="$(docker run --rm --runtime=nvidia --gpus all "${gpu_test_image}" nvidia-smi 2>&1)"
  gpu_test_code=$?
  if [[ "${gpu_test_code}" -eq 0 ]]; then
    ok "GPU visible inside Docker container"
  else
    fail "GPU test container failed with NVIDIA runtime (exit ${gpu_test_code})"
    printf '%s\n' "${gpu_test_output}" | sed 's/^/[GPU]  /'
    echo "[INFO] /etc/docker/daemon.json:"
    if [[ -r /etc/docker/daemon.json ]]; then
      sed 's/^/[INFO] /' /etc/docker/daemon.json
    else
      echo "[INFO] unreadable"
    fi
    echo "[INFO] Docker runtimes: $(docker info --format '{{json .Runtimes}}' 2>/dev/null || echo unavailable)"
    if command -v nvidia-ctk >/dev/null 2>&1; then
      echo "[INFO] nvidia-ctk: $(nvidia-ctk --version 2>&1 | head -n 1)"
    fi
  fi
fi

if [[ "${failures}" -gt 0 ]]; then
  echo
  echo "Doctor result: ${failures} failure(s), ${warnings} warning(s)."
  exit 1
fi

echo
echo "Doctor result: healthy (${warnings} warning(s))."
