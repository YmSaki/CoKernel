# CoKernel

**Human and AI, one notebook, one kernel, one local GPU.**

CoKernel is a local-first Jupyter workbench for sharing the same notebook workspace, the same live kernel state, and local GPU compute between a human in the browser and an AI agent over MCP.

The primary target is a Windows 11 workstation with an NVIDIA GPU. CoKernel runs inside a dedicated WSL2 Ubuntu distribution, uses Docker Engine for a second isolation boundary, exposes JupyterLab only on localhost, and connects the private MCP endpoint to ChatGPT through OpenAI Secure MCP Tunnel.

> Status: experimental v0.1 bootstrap. One workstation, one shared Jupyter environment, one GPU pool.

## Quick start on Windows

Clone or check out this repository on Windows, then run:

```text
install.cmd
```

`install.cmd` is the **first-install and repair/convergence** entrypoint. It requests Administrator privileges and then:

1. enables/updates WSL when needed;
2. creates a dedicated `CoKernel` Ubuntu 24.04 WSL2 distro;
3. creates the Linux user and copies this checkout onto the WSL ext4 filesystem;
4. disables Windows-drive automount and Windows executable interop inside that distro;
5. installs Docker Engine, Git, and NVIDIA Container Toolkit;
6. configures a real WSL loopback socket bridge in front of Docker-published ports;
7. validates GPU/container access;
8. starts Jupyter + MCP, waits for health, and verifies Windows localhost reachability.

If enabling WSL requires a Windows reboot, setup registers itself to resume after the next sign-in. The installer never calls `wsl --unregister` and refuses to overwrite an unrelated WSL distro or unexpected repository path.

After installation:

```text
JupyterLab: http://localhost:8888
WSL shell:  wsl -d CoKernel
Repo:       ~/src/CoKernel
```

See [`docs/WINDOWS_INSTALL.md`](docs/WINDOWS_INSTALL.md) for the complete flow, restart behavior, safety rules, and advanced parameters.

## Updating an existing installation

For normal upgrades, run from the Windows checkout:

```text
update.cmd
```

`update.cmd` is the **steady-state updater**. It does not create or replace WSL. It:

1. refuses to overwrite tracked local Git changes;
2. runs `git pull --ff-only` on the Windows checkout;
3. re-executes the freshly pulled updater so new migrations apply immediately;
4. synchronizes the checkout into WSL ext4 while preserving `.env`, `.env.local`, and `workspace/`;
5. applies idempotent WSL/runtime/network migrations;
6. rebuilds and starts CoKernel;
7. runs smoke tests inside WSL;
8. verifies that Windows can actually connect to the public localhost ports.

Use `install.cmd` again when you want a repair/convergence pass over the WSL/Docker/NVIDIA prerequisites. Use `update.cmd` for routine software updates.

## Architecture

```text
Windows 11
│
│  http://localhost:8888
│  localhostForwarding
▼
WSL2: dedicated CoKernel Ubuntu
├─ Windows drives: not auto-mounted
├─ Windows interop: disabled
├─ systemd socket proxy (real loopback listener)
│  ├─ 127.0.0.1:8888  -> 127.0.0.1:18888
│  ├─ 127.0.0.1:4040  -> 127.0.0.1:14040
│  └─ 127.0.0.1:8080  -> 127.0.0.1:18080
└─ Docker Engine + NVIDIA Container Toolkit
   └─ Compose project: cokernel
      ├─ jupyter
      │  ├─ Docker backend 127.0.0.1:18888 -> container :8888
      │  ├─ JupyterLab
      │  ├─ jupyter-collaboration
      │  ├─ uv workspace
      │  └─ live notebook kernel -> NVIDIA GPU
      ├─ mcp
      │  ├─ Docker backend 127.0.0.1:14040 -> container :4040
      │  ├─ Jupyter MCP Server
      │  └─ CoKernel same-kernel extension
      └─ tunnel
         ├─ Docker backend 127.0.0.1:18080 -> container :8080
         └─ OpenAI Secure MCP Tunnel -> mcp:4040/mcp

Human:   Windows browser -> Windows localhost -> WSL socket proxy -> Docker -> Jupyter
AI:      ChatGPT -> Secure MCP Tunnel -> Jupyter MCP -> same Jupyter session/kernel
Storage: WORKSPACE_DIR -> /workspace
```

The explicit WSL socket-proxy layer is intentional. Docker can implement a loopback published port through Linux NAT rules without creating the userspace listening socket that WSL `localhostForwarding` expects. CoKernel therefore keeps Docker on private WSL-loopback backend ports and gives Windows a real WSL listener on the public ports.

## Design invariants

1. Notebook files under `/workspace` are the source of truth for notebook content.
2. Python project state is `pyproject.toml` + `uv.lock`; `.venv` is reconstructible.
3. Browser and AI use the same Jupyter Server and the same collaborative notebook document.
4. **Connecting the AI to an existing notebook must attach to its already-running Jupyter kernel. CoKernel never silently creates a second kernel in connect mode.**
5. The MCP port is never exposed beyond localhost; ChatGPT reaches it through Secure MCP Tunnel.
6. Windows files, Docker socket, SSH keys, and cluster credentials are not mounted into the workbench.
7. GPU access is explicitly granted only to the Jupyter compute container.
8. Docker backend ports stay on WSL loopback; Windows reaches CoKernel only through explicit WSL loopback proxy sockets.

### Why CoKernel wraps `use_notebook`

Upstream Jupyter MCP Server creates a new kernel when `kernel_id` is omitted from `use_notebook`. That is fine generically, but it would split CoKernel into two RAM states: the browser could have `df` and `model` in one kernel while the AI receives a fresh kernel.

CoKernel ships a small MCP extension that queries Jupyter `/api/sessions` and resolves the existing kernel for the requested notebook. If exactly one matching kernel is not found, it fails with an actionable error instead of guessing or creating another one.

The intended interaction is:

1. Open `research.ipynb` in JupyterLab and let its kernel start.
2. Ask the AI to use/connect to `research.ipynb`.
3. MCP attaches to that exact kernel ID.
4. Variables/imports/models already resident in RAM are shared.

## Prerequisites

For the automated Windows path:

- Windows 11 is the primary target;
- Windows 10 build 19041+ is accepted by the installer;
- NVIDIA GPU supported by CUDA on WSL;
- current NVIDIA **Windows** driver with WSL CUDA support;
- Internet access during bootstrap/build.

For ChatGPT access through the tunnel, also provide:

- an OpenAI Secure MCP Tunnel ID;
- a restricted runtime API key with Tunnels Read + Use.

Do **not** install a Linux NVIDIA display/kernel driver inside WSL. The Windows NVIDIA driver provides the GPU to WSL; CoKernel installs NVIDIA Container Toolkit only.

## Tunnel setup

The Windows bootstrap intentionally succeeds without OpenAI tunnel credentials. Jupyter + MCP can be validated locally first.

Afterward, edit `~/src/CoKernel/.env` inside the dedicated WSL distro and set:

```dotenv
CONTROL_PLANE_TUNNEL_ID=tunnel_...
CONTROL_PLANE_API_KEY=sk-...
```

Use a restricted runtime key for the long-lived tunnel process. Do not put an OpenAI admin key in `.env`.

Then run:

```bash
cd ~/src/CoKernel
./up.sh
```

## Daily use

For routine updates from Windows:

```text
update.cmd
```

To start the already-installed current version without pulling an update:

```bash
wsl -d CoKernel
cd ~/src/CoKernel
./up.sh
```

Useful commands inside the dedicated WSL distro:

```bash
./down.sh                                  # stop services
./logs.sh                                  # follow all logs
./scripts/doctor.sh                        # host/GPU/runtime checks
./scripts/smoke-test.sh                    # service + proxy checks
./scripts/configure-wsl-loopback-proxy.sh  # reconcile Windows/WSL bridge
```

## Workspace

By default, local `./workspace` is mounted at `/workspace` inside Jupyter. Its contents are intentionally not tracked by the CoKernel infrastructure repository.

To use another WSL-native directory, set:

```dotenv
WORKSPACE_DIR=/home/you/projects/my-notebooks
```

If `/workspace/pyproject.toml` does not exist, the Jupyter entrypoint creates a minimal uv project with `ipykernel`. If a project exists, CoKernel runs `uv sync` and registers its `.venv` as the `CoKernel workspace` kernel.

Use normal uv operations from the Jupyter terminal:

```bash
uv add numpy pandas matplotlib
uv add torch
uv sync
```

## Manual WSL setup

The automated `install.cmd` path is preferred. For debugging or custom installations, the Linux-side pieces remain independently usable:

```bash
./scripts/harden-wsl.sh
# terminate/reopen this dedicated WSL distro
./scripts/bootstrap-ubuntu.sh
# terminate/reopen again for docker-group membership
./scripts/init-env.sh
./scripts/configure-wsl-loopback-proxy.sh
./scripts/doctor.sh
./up.sh
./scripts/smoke-test.sh
```

Run these only in a WSL distro dedicated to CoKernel. The hardening step disables Windows-drive automount and Windows executable interop.

## Services

| Service | Windows public localhost | Private WSL Docker backend | Purpose |
|---|---:|---:|---|
| `jupyter` | `127.0.0.1:8888` | `127.0.0.1:18888` | Browser UI, collaborative notebook documents, kernels, GPU execution |
| `mcp` | `127.0.0.1:4040` | `127.0.0.1:14040` | Pinned Jupyter MCP Server + CoKernel same-kernel extension |
| `tunnel` | `127.0.0.1:8080` | `127.0.0.1:18080` | Secure MCP Tunnel health/UI; optional profile |

Both layers bind to loopback only. The public WSL listeners exist so Windows WSL localhost forwarding sees real sockets; Docker never needs to bind these services to `0.0.0.0`.

## Security model

CoKernel reduces blast radius; it is not a malware-analysis sandbox.

```text
Windows host
  -> dedicated WSL2 VM boundary
    -> Docker container boundary
      -> /workspace + GPU
```

The Jupyter container does not receive the Docker socket, Windows drives, your WSL home directory, SSH keys, or cluster credentials. The automated installer uses a passwordless-sudo account at the **WSL host layer** for maintainability; that account is not mounted or exposed to the Jupyter/MCP containers.

See [`docs/SECURITY.md`](docs/SECURITY.md) for the threat model.

## Version pins

Control-path dependencies are intentionally pinned:

- uv: `0.12.12`
- Datalayer Jupyter MCP Server: `2.1.12`
- CoKernel MCP extension: `0.1.0`
- OpenAI tunnel-client: `v0.0.13`

## Project docs

- [`docs/WINDOWS_INSTALL.md`](docs/WINDOWS_INSTALL.md)
- [`docs/ARCHITECTURE.md`](docs/ARCHITECTURE.md)
- [`docs/SECURITY.md`](docs/SECURITY.md)
- [`AGENTS.md`](AGENTS.md)
