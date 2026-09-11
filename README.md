# CoKernel

**Human and AI, one notebook, one kernel, one local GPU.**

CoKernel is a local-first Jupyter workbench for sharing the same notebook workspace, the same live kernel state, and local GPU compute between a human in the browser and an AI agent over MCP.

The primary target is a Windows 11 workstation with an NVIDIA GPU. CoKernel runs inside a dedicated WSL2 Ubuntu distribution, uses Docker Engine for a second isolation boundary, exposes JupyterLab only on localhost, and connects the private MCP endpoint to ChatGPT through OpenAI Secure MCP Tunnel.

> Status: experimental v0.1 bootstrap. One workstation, one shared Jupyter environment, one GPU pool.

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
         │  ├─ uv workspace
         │  └─ live notebook kernel -> NVIDIA GPU
         ├─ mcp
         │  ├─ Jupyter MCP Server :4040
         │  └─ CoKernel same-kernel extension
         └─ tunnel
            └─ OpenAI Secure MCP Tunnel -> mcp:4040/mcp

Human:   Windows browser -> http://localhost:8888
AI:      ChatGPT -> Secure MCP Tunnel -> Jupyter MCP -> same Jupyter session/kernel
Storage: WORKSPACE_DIR -> /workspace
```

## Design invariants

1. Notebook files under `/workspace` are the source of truth for notebook content.
2. Python project state is `pyproject.toml` + `uv.lock`; `.venv` is reconstructible.
3. Browser and AI use the same Jupyter Server and the same collaborative notebook document.
4. **Connecting the AI to an existing notebook must attach to its already-running Jupyter kernel. CoKernel never silently creates a second kernel in connect mode.**
5. The MCP port is never exposed beyond localhost; ChatGPT reaches it through Secure MCP Tunnel.
6. Windows files, Docker socket, SSH keys, and cluster credentials are not mounted into the workbench.
7. GPU access is explicitly granted only to the Jupyter compute container.

### Why CoKernel wraps `use_notebook`

Upstream Jupyter MCP Server creates a new kernel when `kernel_id` is omitted from `use_notebook`. That is fine generically, but it would split CoKernel into two RAM states: the browser could have `df` and `model` in one kernel while the AI receives a fresh kernel.

CoKernel ships a small MCP extension that queries Jupyter `/api/sessions` and resolves the existing kernel for the requested notebook. If exactly one matching kernel is not found, it fails with an actionable error instead of guessing or creating another one.

The intended interaction is therefore:

1. Open `research.ipynb` in JupyterLab and let its kernel start.
2. Ask the AI to use/connect to `research.ipynb`.
3. MCP attaches to that exact kernel ID.
4. Variables/imports/models already resident in RAM are shared.

## Prerequisites

- Windows 11 with current WSL2
- NVIDIA GPU supported by CUDA on WSL
- Current NVIDIA Windows driver with WSL CUDA support
- A dedicated Ubuntu 24.04 WSL distribution (strongly recommended)
- Internet access during bootstrap/build
- For ChatGPT access: an OpenAI Secure MCP Tunnel ID and a restricted runtime API key with Tunnels Read + Use

Do **not** install a Linux NVIDIA display/kernel driver inside WSL. The Windows NVIDIA driver provides the GPU to WSL.

## First-time setup

Clone this repository **inside the WSL ext4 filesystem**, for example under `~/src`. Do not clone it under `/mnt/c` because the recommended hardening disables Windows-drive automounting.

```bash
git clone https://github.com/YmSaki/CoKernel.git
cd CoKernel
```

### 1. Enable the dedicated WSL configuration

If systemd is not already active, run this first. Only do it in a WSL distribution dedicated to CoKernel:

```bash
./scripts/harden-wsl.sh
```

It configures `/etc/wsl.conf` to enable systemd while disabling Windows-drive automount and Windows executable interop. Then terminate that WSL distribution from PowerShell and reopen it:

```powershell
wsl --terminate <your-cokernel-distro-name>
```

### 2. Install Docker Engine and NVIDIA Container Toolkit

```bash
./scripts/bootstrap-ubuntu.sh
```

The script installs Docker Engine from Docker's official Ubuntu repository and NVIDIA Container Toolkit from NVIDIA's repository. It does not install an NVIDIA Linux driver. Restart the WSL distribution once afterward so Docker group membership is refreshed.

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

If both OpenAI tunnel variables are present, `up.sh` also starts the tunnel profile. If absent, Jupyter + MCP still start locally.

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

## Services

| Service | Host exposure | Purpose |
|---|---:|---|
| `jupyter` | `127.0.0.1:8888` | Browser UI, collaborative notebook documents, kernels, GPU execution |
| `mcp` | `127.0.0.1:4040` | Pinned Jupyter MCP Server + CoKernel same-kernel extension |
| `tunnel` | `127.0.0.1:8080` | Secure MCP Tunnel health/UI; optional profile |

All published ports bind to loopback only.

## Security model

CoKernel reduces blast radius; it is not a malware-analysis sandbox.

```text
Windows host
  -> dedicated WSL2 VM boundary
    -> Docker container boundary
      -> /workspace + GPU
```

The Jupyter container does not receive the Docker socket, Windows drives, your home directory, SSH keys, or cluster credentials. See [`docs/SECURITY.md`](docs/SECURITY.md).

## Version pins

Control-path dependencies are intentionally pinned:

- uv: `0.12.12`
- Datalayer Jupyter MCP Server: `2.1.12`
- CoKernel MCP extension: `0.1.0`
- OpenAI tunnel-client: `v0.0.8--context-conduit-emerald`

## Project docs

- [`docs/ARCHITECTURE.md`](docs/ARCHITECTURE.md)
- [`docs/SECURITY.md`](docs/SECURITY.md)
- [`AGENTS.md`](AGENTS.md)
