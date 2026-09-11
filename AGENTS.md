# CoKernel agent instructions

## Product intent

CoKernel makes one local Jupyter workspace usable by both a human browser session and an AI agent while execution remains on the user's local GPU workstation.

## Non-negotiable invariants

- `/workspace` is the only user-workspace mount required by the Jupyter container.
- Do not mount `/var/run/docker.sock` into any CoKernel service.
- Do not mount Windows drives, the WSL user's entire home directory, SSH directories, cloud credentials, kubeconfig, or Rancher credentials.
- Jupyter and MCP host ports must bind to `127.0.0.1`, never `0.0.0.0`.
- The Secure MCP Tunnel must target the private Compose-network MCP URL, not a public host port.
- Long-lived OpenAI tunnel processes use a restricted runtime key. Never require an OpenAI admin key at runtime.
- Keep Jupyter authentication and MCP authentication as separate secrets.
- Never install a Linux NVIDIA kernel/display driver in WSL. GPU access comes from the Windows driver; containers use NVIDIA Container Toolkit.
- Prefer uv (`pyproject.toml` + `uv.lock`) for Python project state.
- **Split-brain kernels are a correctness bug.** In `use_notebook(mode="connect")`, omission of `kernel_id` must resolve the already-running Jupyter session for that notebook and attach to its kernel. Never silently create a second kernel. If zero or multiple distinct kernels match, fail closed with an actionable error.

## Compatibility target

Primary target:

- Windows 11
- current WSL2
- dedicated Ubuntu 24.04 distribution
- NVIDIA GPU
- Docker Engine inside WSL
- NVIDIA Container Toolkit
- Docker Compose v2

Native Linux is allowed as a secondary target if these invariants remain intact.

## Change policy

Changes to `compose.yaml`, authentication, WSL hardening, GPU runtime configuration, same-kernel session resolution, or tunnel-client configuration are security/correctness-sensitive. Document the reason and update `docs/ARCHITECTURE.md` or `docs/SECURITY.md` when a trust or execution boundary changes.

Do not silently switch pinned control-path dependencies to floating `latest` tags.

## Acceptance path

A meaningful change should preserve this flow:

```bash
./scripts/doctor.sh
./up.sh
./scripts/smoke-test.sh
```

The expected end state is:

1. JupyterLab is reachable from the Windows browser at localhost.
2. A workspace kernel exists from the uv environment.
3. The Jupyter container can access the NVIDIA GPU.
4. The MCP server is healthy and requires its bearer token.
5. The CoKernel `use_notebook` wrapper is installed.
6. When tunnel credentials are configured, the tunnel runtime reports ready and ChatGPT can enumerate Jupyter MCP tools.
7. A notebook opened in JupyterLab is attached by MCP to that same existing kernel, so browser-created in-memory variables are visible to MCP execution.
8. Notebook edits/executions performed through MCP are visible in JupyterLab.
