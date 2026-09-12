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
10. configures a systemd socket-proxy bridge between Windows localhost and private Docker backend ports;
11. starts Jupyter + MCP with `up.sh` unless `-NoStart` is supplied;
12. waits for service health and verifies Windows can connect to the localhost endpoints.

If enabling WSL requires a Windows reboot, the installer registers itself in `RunOnce`. Restart Windows and accept the UAC prompt after sign-in; setup continues from the beginning and skips already-completed safe steps.

## Install versus update

`install.cmd` is intentionally idempotent and remains the **first-install and repair/convergence** entrypoint. Rerunning it on a managed `CoKernel` distro is supported and does not unregister or replace the distro.

For routine upgrades after installation, use:

```text
update.cmd
```

`update.cmd` does not recreate WSL or reinstall the base platform. It:

1. checks that tracked files in the Windows checkout are clean;
2. runs `git pull --ff-only`;
3. re-executes the freshly pulled updater so new migrations apply immediately;
4. synchronizes the Windows checkout into the WSL ext4 runtime copy while preserving `.env`, `.env.local`, and `workspace/`;
5. reconciles WSL-side runtime/network configuration;
6. rebuilds and starts CoKernel;
7. runs smoke tests inside WSL;
8. verifies Windows localhost reachability.

Use `update.cmd` for normal updates. Use `install.cmd` again when base WSL/Docker/NVIDIA provisioning needs repair or convergence.

## Why there is a WSL loopback proxy

Windows, WSL2, and Docker have distinct network namespaces. Docker can publish a port to WSL `127.0.0.1` using Linux NAT rules without creating the userspace listening socket that WSL `localhostForwarding` expects to mirror to Windows.

CoKernel therefore separates public and backend ports:

```text
Windows localhost:8888
  -> WSL localhostForwarding
  -> systemd socket proxy 127.0.0.1:8888
  -> Docker backend 127.0.0.1:18888
  -> Jupyter container :8888
```

The same pattern is used for MCP (`4040 -> 14040`) and the optional tunnel health UI (`8080 -> 18080`). Both layers remain loopback-only; CoKernel does not bind these services to `0.0.0.0` merely to make Windows access work.

## Safety properties

The installer is intentionally non-destructive:

- it never calls `wsl --unregister`;
- if a distro named `CoKernel` already exists without CoKernel's management marker, setup stops instead of modifying it;
- if the WSL repository target unexpectedly exists before the seed marker is present, setup stops instead of overwriting it;
- the Windows source checkout is copied before Windows-drive automount/interop are disabled;
- `.env` and `.env.local` from the Windows checkout are not carried into the new runtime;
- later update synchronization preserves WSL `.env`, `.env.local`, and `workspace/`;
- the runtime checkout lives on WSL's ext4 filesystem, not `/mnt/c`;
- Windows localhost acceptance is checked before `install.cmd` or `update.cmd` reports success.

## Resulting layout

```text
Windows 11
└─ WSL2 distro: CoKernel
   ├─ systemd loopback proxy sockets
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

The Windows-side `update.cmd` path uses the Windows checkout and its existing GitHub authentication, then synchronizes source files into WSL. This avoids requiring Windows credentials inside the workbench containers.

If you also choose to use Git directly inside the WSL host, configure a WSL-local SSH key or GitHub CLI/PAT there. WSL-host credentials are never mounted into the Jupyter/MCP containers.
