# CoKernel architecture

## Context

CoKernel is a single-workstation collaborative compute environment. The human operates JupyterLab in a Windows browser. The AI reaches a Jupyter MCP server through OpenAI Secure MCP Tunnel. Both paths terminate at the same Jupyter Server, notebook document layer, and—by CoKernel policy—the same already-running notebook kernel.

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
  |                            | Jupyter REST/WebSocket/Yjs
  v                            v
JupyterLab :8888 ---------- Jupyter Server
                               |
                         /api/sessions
                               |
                CoKernel session-attach wrapper
                               |
                               v
                    existing notebook kernel
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

## Same-kernel invariant

The upstream Jupyter MCP `use_notebook` contract creates a new kernel when `kernel_id` is omitted. That default is unsafe for CoKernel because the browser and AI could edit the same notebook file while executing against different in-memory Python states.

CoKernel installs the `zz-cokernel-session-attach` Jupyter MCP extension. It wraps the upstream `use_notebook` tool:

1. `mode="create"` retains upstream behavior.
2. `mode="connect"` with an explicit `kernel_id` retains the caller's explicit choice.
3. `mode="connect"` without `kernel_id` queries the configured Jupyter Server's `/api/sessions` endpoint.
4. Exactly one kernel serving the requested notebook path is required.
5. That kernel ID is passed to the upstream `use_notebook` implementation.
6. Zero matches fail with instructions to open/start the notebook in JupyterLab.
7. Multiple distinct matches fail as ambiguous.

This is fail-closed by design: CoKernel never silently creates a second kernel while connecting to an existing notebook.

## Notebook state

Notebook files live under `/workspace`. `jupyter-collaboration` supplies the real-time document layer required for human/agent edits to converge. In standalone MCP mode, Jupyter MCP uses the collaborative document path so cell edits land in the same notebook document shown in JupyterLab.

The Jupyter MCP server points at the same Jupyter server used by the browser:

```text
JUPYTER_URL=http://jupyter:8888
```

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
3. Build/start Jupyter and the CoKernel-patched Jupyter MCP service.
4. If both OpenAI tunnel credentials are configured, activate the Compose `tunnel` profile.
5. Jupyter becomes healthy before MCP starts; MCP becomes healthy before the tunnel starts.

## Future extensions

Candidates for later versions, not v0.1 requirements:

- end-to-end automated proof that a browser-created variable is visible through MCP
- durable server-side notebook execution with `jupyter-server-nbmodel`
- project/workspace switching
- per-job GPU allocation on multi-GPU hosts
- resource ceilings for CPU/RAM/GPU
- backups/snapshots of notebook workspaces
- RKE2/Rancher deployment for multi-workbench scheduling
