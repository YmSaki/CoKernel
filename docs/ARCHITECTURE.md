# CoKernel architecture

## Context

CoKernel is a single-workstation collaborative compute environment. The human operates JupyterLab in a Windows browser. The AI reaches a Jupyter MCP server through OpenAI Secure MCP Tunnel. Both paths terminate at the same Jupyter Server and therefore the same notebook document/session layer.

## Component model

```text
                         OpenAI control plane
                               ^
                               | outbound HTTPS
                               |
Windows 11                tunnel-client
Browser                       |
  | localhost                 | private Compose network
  v                           v
WSL2 -------------------- Jupyter MCP :4040
  |                            |
  |                            | Jupyter REST/WebSocket/RTC
  v                            v
JupyterLab :8888 ---------- Jupyter Server
                               |
                               v
                         workspace kernels
                               |
                               v
                         NVIDIA GPU (WSL GPU-PV)
```

## Boundaries

### Windows -> WSL2

WSL2 is the first isolation boundary. The recommended dedicated distro disables DrvFs automount and Windows executable interop. GPU paravirtualization remains enabled.

### WSL2 -> Docker

Docker is the second isolation boundary. The Jupyter container receives only a workspace bind mount and GPU access. It does not receive the Docker socket or host credentials.

### Human path

`jupyter` publishes container port 8888 to `127.0.0.1` in WSL. WSL localhost forwarding makes that available to the Windows browser as `http://localhost:8888`.

### AI path

`mcp` is reachable on the private Compose network and additionally binds to WSL loopback for local diagnostics. `tunnel-client` addresses `http://mcp:4040/mcp` directly. No inbound Internet listener is required.

The tunnel runtime injects a static MCP `Authorization` header from an environment-backed secret reference. The MCP bearer token is not stored in checked-in configuration.

## Notebook state

Notebook files live under `/workspace`. `jupyter-collaboration` supplies the real-time document layer required for human/agent edits to converge.

The Jupyter MCP server points at the same Jupyter server used by the browser:

```text
JUPYTER_URL=http://jupyter:8888
```

This is an architectural invariant. Do not create an independent execution backend for the MCP path unless the product intentionally moves to a remote-compute architecture.

## Python state

CoKernel separates infrastructure Python from workspace Python.

- `/opt/cokernel`: image-built environment used to run JupyterLab.
- `/workspace/.venv`: user project environment managed by uv.
- `/workspace/pyproject.toml` + `/workspace/uv.lock`: reproducible workspace dependency state.

At startup, CoKernel runs `uv sync` when enabled and registers `/workspace/.venv` as the `CoKernel workspace` kernel.

## GPU path

The Windows NVIDIA driver exposes the GPU to WSL through GPU paravirtualization. Docker Engine inside WSL uses NVIDIA Container Toolkit. Compose grants the Jupyter service GPU access with `gpus: all`.

No Linux NVIDIA kernel/display driver is installed in WSL.

## Startup model

`./up.sh` is the stable operator interface.

1. Ensure `.env` exists and local tokens have been generated.
2. Confirm Docker is reachable.
3. Start Jupyter and MCP.
4. If both OpenAI tunnel credentials are configured, activate the Compose `tunnel` profile.
5. Jupyter becomes healthy before MCP starts; MCP becomes healthy before the tunnel starts.

## Future extensions

Candidates for later versions, not v0.1 requirements:

- explicit notebook-session attachment tests to prove browser and agent are on the same kernel
- durable server-side notebook execution with `jupyter-server-nbmodel`
- project/workspace switching
- per-job GPU allocation on multi-GPU hosts
- resource ceilings for CPU/RAM/GPU
- backups/snapshots of notebook workspaces
- RKE2/Rancher deployment for multi-workbench scheduling
