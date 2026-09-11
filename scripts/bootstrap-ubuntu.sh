#!/usr/bin/env bash
set -euo pipefail

if ! grep -qi microsoft /proc/sys/kernel/osrelease 2>/dev/null; then
  echo "Warning: this bootstrap is designed for Ubuntu under WSL2."
fi

if [[ "$(id -u)" -eq 0 ]]; then
  echo "Run this script as your normal WSL user; it will invoke sudo when needed."
  exit 1
fi

read_os_release_value() {
  local key="$1"
  awk -F= -v key="${key}" '
    $1 == key {
      value = substr($0, index($0, "=") + 1)
      gsub(/^"|"$/, "", value)
      print value
      exit
    }
  ' /etc/os-release
}

OS_ID="$(read_os_release_value ID)"
VERSION_CODENAME="$(read_os_release_value VERSION_CODENAME)"
UBUNTU_CODENAME="$(read_os_release_value UBUNTU_CODENAME)"
CODENAME="${UBUNTU_CODENAME:-${VERSION_CODENAME}}"

if [[ "${OS_ID}" != "ubuntu" ]]; then
  echo "Unsupported distribution: ${OS_ID:-unknown}. Ubuntu 24.04 is the primary target."
  exit 1
fi

if [[ -z "${CODENAME}" ]]; then
  echo "Could not determine the Ubuntu codename from /etc/os-release."
  exit 1
fi

if ! command -v systemctl >/dev/null 2>&1 || [[ "$(ps -p 1 -o comm= 2>/dev/null)" != "systemd" ]]; then
  echo "systemd is not active in this WSL distribution."
  echo "Run ./scripts/harden-wsl.sh, terminate the distro from PowerShell, reopen it, then rerun bootstrap."
  exit 1
fi

echo "[1/4] Installing Docker Engine prerequisites"
sudo apt-get update
sudo apt-get install -y ca-certificates curl gnupg
sudo install -m 0755 -d /etc/apt/keyrings

if [[ ! -f /etc/apt/keyrings/docker.asc ]]; then
  sudo curl -fsSL https://download.docker.com/linux/ubuntu/gpg -o /etc/apt/keyrings/docker.asc
  sudo chmod a+r /etc/apt/keyrings/docker.asc
fi

sudo tee /etc/apt/sources.list.d/docker.sources >/dev/null <<EOF
Types: deb
URIs: https://download.docker.com/linux/ubuntu
Suites: ${CODENAME}
Components: stable
Architectures: $(dpkg --print-architecture)
Signed-By: /etc/apt/keyrings/docker.asc
EOF

sudo apt-get update
sudo apt-get install -y docker-ce docker-ce-cli containerd.io docker-buildx-plugin docker-compose-plugin
sudo systemctl enable --now docker

echo "[2/4] Installing NVIDIA Container Toolkit"
curl -fsSL https://nvidia.github.io/libnvidia-container/gpgkey \
  | sudo gpg --dearmor --yes -o /usr/share/keyrings/nvidia-container-toolkit-keyring.gpg
curl -s -L https://nvidia.github.io/libnvidia-container/stable/deb/nvidia-container-toolkit.list \
  | sed 's#deb https://#deb [signed-by=/usr/share/keyrings/nvidia-container-toolkit-keyring.gpg] https://#g' \
  | sudo tee /etc/apt/sources.list.d/nvidia-container-toolkit.list >/dev/null
sudo apt-get update
sudo apt-get install -y nvidia-container-toolkit
sudo nvidia-ctk runtime configure --runtime=docker
sudo systemctl restart docker

if ! id -nG "${USER}" | grep -qw docker; then
  echo "[3/4] Adding ${USER} to docker group"
  sudo usermod -aG docker "${USER}"
else
  echo "[3/4] ${USER} already belongs to docker group"
fi

echo "[4/4] Verifying GPU container access"
if command -v nvidia-smi >/dev/null 2>&1; then
  nvidia-smi
elif [[ -x /usr/lib/wsl/lib/nvidia-smi ]]; then
  /usr/lib/wsl/lib/nvidia-smi
else
  echo "NVIDIA GPU is not visible inside WSL."
  echo "Install/update the NVIDIA Windows driver with WSL CUDA support; do not install a Linux NVIDIA driver here."
  exit 1
fi

sudo docker run --rm --gpus all nvidia/cuda:12.8.1-base-ubuntu24.04 nvidia-smi

echo
echo "Bootstrap complete."
echo "Restart this WSL distribution so docker group membership is refreshed."
echo "If you have not hardened this dedicated distro yet, run ./scripts/harden-wsl.sh before that restart."
