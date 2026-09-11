# CoKernel

**Human and AI, one notebook, one kernel, one local GPU.**

CoKernel is a local-first Jupyter workbench for sharing the same notebook workspace and GPU compute between a human in the browser and an AI agent over MCP.

The primary target is a Windows 11 workstation with an NVIDIA GPU. CoKernel runs inside a dedicated WSL2 Ubuntu distribution, uses Docker Engine for a second isolation boundary, exposes JupyterLab only on localhost, and connects the private MCP endpoint to ChatGPT through OpenAI Secure MCP Tunnel.

> Status: experimental v0.1 bootstrap. The architecture is intentionally small: one workstation, one shared Jupyter environment, one GPU pool.

## Architecture

```text
Windows 11
└─ WSL2: dedicated CoKernel Ubuntu
   ├─ Windows drives: not auto-mounted (recommended)
   ├─ Windows interop: disabled (recommended)
   └─ Docker Engine + NVIDIA Container Toolkit
      └─ Compose project: cokernel
         ├─ jupyter
         │  ├─ JupyterLab :8888
         │  ├─ jupyter-collaboration
         │  ├─ uv
         │  └─ workspace kernel -> NVIDIA GPU
         ├─ mcp
         │  └─ Jupyter MCP Server :4040
         └─ tunnel
            └─ OpenAI Secure MCP Tunnel -> mcp:4040/mcp

Human:   Windows browser -> http://localhost:8888
AI:      ChatGPT -> Secure MCP Tunnel -> Jupyter MCP -> same Jupyter server
Storage: WORKSPACE_DIR -> /workspace
```

## Design invariants

1. The notebook files under `/workspace` are the source of truth for notebook state.
2. Python project state is represented by `pyproject.toml` and `uv.lock`; `.venv` is reconstructible.
3. Jupyter and the MCP server talk to the same Jupyter server and its sessions/kernels.
4. The MCP port is never exposed beyond localhost; ChatGPT reaches it through Secure MCP Tunnel.
5. The Windows filesystem, Docker socket, SSH keys, and Kubernetes/Rancher credentials are not mounted into the workbench.
6. GPU access is explicitly granted to the Jupyter container with Docker's NVIDIA runtime.

## Prerequisites

- Windows 11 with current WSL2
- NVIDIA GPU supported by CUDA on WSL
- Current NVIDIA Windows production driver with WSL CUDA support
- A dedicated Ubuntu 24.04 WSL distribution is strongly recommended
- Internet access during bootstrap/build
- For ChatGPT access: an OpenAI Secure MCP Tunnel ID and a restricted runtime API key with Tunnels Read + Use

Do **not** install a Linux NVIDIA display/kernel driver inside WSL. The Windows NVIDIA driver provides the GPU to WSL.

## First-time setup

Clone this repository **inside the WSL ext4 filesystem**, for example under `~/src`. Do not clone it under `/mnt/c`, because the recommended hardening disables Windows-drive automounting.

```bash
git clone https://github.com/YmSaki/CoKernel.git
cd CoKernel
```

### 1. Install Docker Engine and NVIDIA Container Toolkit

```bash
./scripts/bootstrap-ubuntu.sh
```

The script installs Docker Engine from Docker's official Ubuntu repository and NVIDIA Container Toolkit from NVIDIA's repository. It does not install an NVIDIA Linux driver.

### 2. Harden the dedicated WSL distribution

Only do this in a WSL distribution dedicated to CoKernel:

```bash
./scripts/harden-wsl.sh
```

This configures `/etc/wsl.conf` to:

- enable systemd
- disable automatic Windows-drive mounts
- disable launching Windows executables from Linux
- remove Windows paths from Linux `$PATH`
- keep WSL GPU paravirtualization enabled

Then terminate that WSL distribution from PowerShell and reopen it:

```powershell
wsl --terminate <your-cokernel-distro-name>
```

### 3. Create local secrets

```bash
./scripts/init-env.sh
```

`JUPYTER_TOKEN` and `MCP_TOKEN` are generated locally. `.env` is git-ignored.

For ChatGPT connectivity, edit `.env` and set:

```dotenv
CONTROL_PLANE_TUNNEL_ID=tunnel_...
CONTROL_PLANE_API_KEY=sk-...
```

Use a **restricted runtime key** for the long-lived tunnel process. Do not put an OpenAI admin key in `.env`.

### 4. Verify the machine

```bash
./scripts/doctor.sh
```

The doctor checks WSL2, Docker Compose, the WSL isolation settings, NVIDIA visibility, and a GPU-enabled test container.

### 5. Start CoKernel

```bash
./up.sh
```

Then open:

```text
http://localhost:8888
```

If both OpenAI tunnel variables are present, `up.sh` also starts the tunnel profile. If they are absent, Jupyter + MCP still start locally.

## Daily use

```bash
git pull
./up.sh
```

That is the intended steady-state workflow.

Useful commands:

```bash
./down.sh             # stop services
./logs.sh             # follow all logs
./scripts/doctor.sh   # re-run host checks
./scripts/smoke-test.sh
```

## Workspace

By default, the local `./workspace` directory is mounted at `/workspace` inside Jupyter. Its contents are intentionally not tracked by the CoKernel repository.

To use another WSL-native directory, set this in `.env`:

```dotenv
WORKSPACE_DIR=/home/you/projects/my-notebooks
```

If `/workspace/pyproject.toml` does not exist, the Jupyter entrypoint creates a minimal uv project with `ipykernel`. If a project already exists, CoKernel runs `uv sync` and registers its `.venv` as the `CoKernel workspace` kernel.

Use normal uv project operations from the Jupyter terminal:

```bash
uv add numpy pandas matplotlib
uv add torch
uv sync
```

## Services

| Service | Host exposure | Purpose |
|---|---:|---|
| `jupyter` | `127.0.0.1:8888` | Browser UI, notebook documents, kernels, GPU execution |
| `mcp` | `127.0.0.1:4040` | Jupyter MCP server; localhost diagnostics only |
| `tunnel` | `127.0.0.1:8080` | Secure MCP Tunnel health/UI; optional profile |

All published ports bind to loopback only.

## Security model

CoKernel reduces blast radius; it is not a malware-analysis sandbox.

The intended boundary is:

```text
Windows host
  -> dedicated WSL2 VM boundary
    -> Docker container boundary
      -> /workspace + GPU
```

The Jupyter container does not receive the Docker socket, Windows drives, your home directory, SSH keys, or cluster credentials. See [`docs/SECURITY.md`](docs/SECURITY.md) for the threat model.

## Version pins

The initial v0.1 defaults pin major infrastructure that sits on the control path. Override image variables in `.env` only when intentionally upgrading.

- uv: `0.12.12` in the Jupyter image build
- Datalayer Jupyter MCP Server: `1.3.2`
- OpenAI tunnel-client: `v0.0.8--context-conduit-emerald`

## Project docs

- [`docs/ARCHITECTURE.md`](docs/ARCHITECTURE.md)
- [`docs/SECURITY.md`](docs/SECURITY.md)
- [`AGENTS.md`](AGENTS.md)
