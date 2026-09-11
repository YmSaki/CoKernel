# CoKernel security model

## Summary

CoKernel intentionally gives an AI agent the ability to edit notebooks and execute code on local compute. Treat that as remote code execution inside a constrained workbench.

The objective is blast-radius reduction, not adversarial malware containment.

## Trust assumptions

Trusted:

- Windows host and its NVIDIA driver
- WSL implementation and kernel
- Docker Engine / container runtime
- CoKernel repository configuration
- OpenAI Secure MCP Tunnel control path

Partially trusted / potentially destructive:

- notebook code
- Python packages installed into the workspace
- commands initiated through MCP
- AI-generated code

## Host isolation

A dedicated WSL distribution is recommended. `/etc/wsl.conf` should disable:

- automatic Windows-drive mounts
- Windows executable interop
- appending the Windows PATH

This prevents ordinary notebook code from trivially walking `C:\Users`, invoking `powershell.exe`, or reusing Windows CLI credentials.

GPU paravirtualization stays enabled.

## Container isolation

The Jupyter container receives:

- `/workspace`
- network access
- GPU access
- normal container CPU/RAM access

It must not receive:

- `/var/run/docker.sock`
- `/`
- `/home/<host-user>`
- `~/.ssh`
- cloud-provider credentials
- Kubernetes/RKE2 kubeconfig
- Rancher credentials
- arbitrary Windows/DrvFs mounts

`no-new-privileges` is enabled on services. More restrictive seccomp/capability profiles may be added after GPU/Jupyter compatibility testing.

## Network exposure

Jupyter, MCP, and tunnel health ports bind to loopback only.

Do not change:

```text
127.0.0.1:<port>:<container-port>
```

to a wildcard host binding unless a separate authenticated reverse proxy and explicit threat review are introduced.

The AI path does not require inbound Internet exposure. `tunnel-client` initiates outbound control-plane traffic and forwards tunnel commands to the private MCP endpoint.

## Authentication

Three secrets have distinct purposes:

- `JUPYTER_TOKEN`: browser/MCP-to-Jupyter authentication
- `MCP_TOKEN`: tunnel-to-MCP authentication
- `CONTROL_PLANE_API_KEY`: tunnel-client authentication to OpenAI

Do not reuse one value for multiple roles.

The runtime OpenAI key should be restricted to Tunnels Read + Use. OpenAI admin keys are not runtime secrets and must not be placed in CoKernel `.env`.

`.env` is local-only and git-ignored.

## Dependency risk

Notebook environments are user-controlled. Installing a package is equivalent to running third-party code in the workbench container. A malicious package can read all files in `/workspace`, use the GPU, consume compute resources, and make outbound network requests.

It should not be able to directly access Windows files or control Docker when the recommended boundaries are intact.

## Non-goals

v0.1 does not claim protection against:

- WSL or Windows kernel escapes
- Docker/container-runtime escapes
- GPU driver vulnerabilities
- side-channel attacks
- malicious code intentionally targeting virtualization boundaries
- denial of service against the local CPU/RAM/GPU/disk/network

Use a stronger disposable VM or dedicated physical host for hostile malware analysis.

## Incident response

If the workbench behaves unexpectedly:

```bash
./down.sh
docker compose --profile tunnel down
```

Then terminate the dedicated WSL distribution from PowerShell. Rotate `MCP_TOKEN`, `JUPYTER_TOKEN`, and the OpenAI runtime key if exposure is suspected. Inspect the workspace before restarting.
