#!/usr/bin/env bash
set -euo pipefail

if ! grep -qi microsoft /proc/sys/kernel/osrelease 2>/dev/null; then
  echo "This script only configures WSL distributions."
  exit 1
fi

if [[ "$(id -u)" -eq 0 ]]; then
  echo "Run this script as the normal user that should remain the WSL default user."
  exit 1
fi

assume_yes=0
if [[ "${1:-}" == "--yes" ]]; then
  assume_yes=1
fi

if [[ "${assume_yes}" -ne 1 ]]; then
  cat <<'EOF'
This hardening is intended ONLY for a dedicated CoKernel WSL distribution.
It disables automatic Windows-drive mounts and Windows executable interop.

Type DEDICATED to continue.
EOF
  read -r answer
  if [[ "${answer}" != "DEDICATED" ]]; then
    echo "Cancelled."
    exit 1
  fi
fi

if [[ -f /etc/wsl.conf ]]; then
  backup="/etc/wsl.conf.cokernel-backup-$(date +%Y%m%d%H%M%S)"
  sudo cp /etc/wsl.conf "${backup}"
  echo "Backed up existing config to ${backup}"
fi

sudo tee /etc/wsl.conf >/dev/null <<EOF
[boot]
systemd=true

[automount]
enabled=false
mountFsTab=false

[interop]
enabled=false
appendWindowsPath=false

[gpu]
enabled=true

[user]
default=${USER}
EOF

cat <<EOF
CoKernel WSL hardening written to /etc/wsl.conf.

Terminate and reopen this distribution for it to take effect.
From PowerShell:
  wsl --terminate ${WSL_DISTRO_NAME:-<distro-name>}

After reopening, run:
  cd <CoKernel checkout>
  ./scripts/doctor.sh
EOF
