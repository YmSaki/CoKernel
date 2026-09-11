# Windows one-click bootstrap

CoKernel's primary installation path is a Windows-side bootstrap that creates and configures a dedicated WSL2 environment.

## Quick path

From a Windows checkout of this repository, run:

```text
install.cmd
```

The launcher invokes `install.ps1` and requests Administrator privileges through UAC.

The installer performs these steps:

1. verifies the Windows build and WSL availability;
2. enables/updates WSL when necessary;
3. installs `Ubuntu-24.04` as a named WSL2 distribution called `CoKernel` under `%LOCALAPPDATA%\CoKernel\WSL`;
4. creates a dedicated Linux user;
5. copies the current repository checkout into `/home/<user>/src/CoKernel` on the WSL ext4 filesystem;
6. applies CoKernel's WSL isolation policy;
7. restarts only the CoKernel WSL VM so systemd and isolation settings take effect;
8. installs Docker Engine, Git, and NVIDIA Container Toolkit;
9. validates WSL/GPU/container access with `doctor.sh`;
10. starts Jupyter + MCP with `up.sh` unless `-NoStart` is supplied.

If enabling WSL requires a Windows reboot, the installer registers itself in `RunOnce`. Restart Windows and accept the UAC prompt after sign-in; setup continues from the beginning and skips already-completed safe steps.

## Safety properties

The installer is intentionally non-destructive:

- it never calls `wsl --unregister`;
- if a distro named `CoKernel` already exists without CoKernel's management marker, setup stops instead of modifying it;
- if the WSL repository target unexpectedly exists before the seed marker is present, setup stops instead of overwriting it;
- the Windows source checkout is copied before Windows-drive automount/interop are disabled;
- `.env` and `.env.local` from the Windows checkout are not carried into the new runtime;
- the runtime checkout lives on WSL's ext4 filesystem, not `/mnt/c`.

## Resulting layout

```text
Windows 11
└─ WSL2 distro: CoKernel
   └─ /home/<user>/src/CoKernel
      ├─ .env                  # local secrets, generated inside WSL
      ├─ workspace/            # local notebook state
      ├─ compose.yaml
      └─ ...
```

The distro is installed by default at:

```text
%LOCALAPPDATA%\CoKernel\WSL
```

The installer does not change the user's default WSL distribution.

## Windows restart behavior

A Windows restart can be required only when WSL/Virtual Machine Platform must first be enabled. Normal CoKernel setup also terminates and restarts the **CoKernel WSL distro itself** a few times; those restarts are automatic and do not reboot Windows.

## Advanced PowerShell usage

Instead of `install.cmd`, run `install.ps1` directly to override defaults:

```powershell
powershell -ExecutionPolicy Bypass -File .\install.ps1 `
  -DistroName CoKernel `
  -Distribution Ubuntu-24.04 `
  -InstallLocation "$env:LOCALAPPDATA\CoKernel\WSL" `
  -LinuxUser cokernel
```

Useful switches:

```text
-RestartNow   reboot Windows immediately if WSL enablement requires it
-NoStart      install and validate, but do not start the Compose services
```

## GPU prerequisite

The Windows host must have an NVIDIA driver with WSL CUDA support. Do not install a Linux NVIDIA display/kernel driver inside the WSL distro. CoKernel installs NVIDIA Container Toolkit only; the GPU device is supplied by the Windows/WSL GPU virtualization path.

## Private-repository authentication

The installer can seed the Git checkout from the Windows copy without exposing Windows credentials to the CoKernel containers. Because this repository is private, future `git pull` operations inside WSL still require one-time GitHub authentication inside that distro (for example a WSL-local SSH key or GitHub CLI/PAT).

Git/SSH credentials remain at the WSL host layer and are never mounted into the Jupyter/MCP containers.
