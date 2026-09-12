# CoKernel Desktop v1

CoKernel Desktop is the Windows control plane for the existing CoKernel WSL compute runtime.

## Product rule

WSL is an implementation detail, not a user workflow. The desktop application owns runtime lifecycle, health, metrics, tunnel configuration, update/repair entrypoints, notifications, and the Windows-facing experience. Linux Python, JAX, PyTorch, CUDA workloads, Jupyter, MCP, and containers remain inside the dedicated CoKernel WSL distribution.

## Current v1 foundation

The desktop application currently provides:

- tray-resident Windows UI;
- Start / Stop / Restart lifecycle actions;
- explicit WSL lifetime ownership via `runtime.ps1`;
- Jupyter launch action;
- component health for WSL, Docker, Jupyter, MCP, and Secure MCP Tunnel;
- active Jupyter kernel count;
- CPU, Windows RAM, WSL RAM, system storage, GPU utilization, VRAM, temperature, and power metrics;
- Docker/CoKernel log viewer;
- redacted diagnostics bundle export;
- update guard when active kernels exist;
- repair launcher using the proven bootstrap path;
- sign-in auto-start through the current-user Run registry key;
- desired runtime state persistence;
- Secure MCP Tunnel settings stored with Windows DPAPI and injected to WSL through stdin;
- an Inno Setup packaging definition that bundles the Windows application and the Linux runtime payload without deleting the WSL data on uninstall.

## Build

```powershell
cd apps\desktop
.\publish.ps1
```

The published executable is written to `dist\desktop`.

To build the installer after publishing:

```powershell
& 'C:\Program Files (x86)\Inno Setup 6\ISCC.exe' installer\windows\CoKernel.iss
```

CI builds the application and `CoKernelSetup.exe` on Windows.

## Security

The desktop process runs as the current Windows user. Elevation is requested only by the existing provisioning/repair flow when Windows/WSL changes require it. Tunnel API keys are not stored in desktop settings as plaintext; they are protected with DPAPI for the current Windows user. The runtime still uses its WSL-local `.env` because Docker Compose consumes it, but Desktop injects the value over stdin rather than placing the key on a Windows command line.

The installer deliberately does not unregister or delete the dedicated WSL distribution during uninstall.

## Compatibility

The existing CLI remains the recovery/developer interface:

```text
install.cmd
update.cmd
start.cmd
stop.cmd
status.cmd
```

Desktop delegates to the same battle-tested runtime contracts instead of introducing a second provisioning implementation.
