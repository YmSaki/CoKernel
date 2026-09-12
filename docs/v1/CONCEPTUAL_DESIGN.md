# CoKernel v1 Conceptual Design

Status: **design baseline**  
Scope: **CoKernel Desktop v1.0**  
Out of scope: multi-node Fabric scheduling, headless remote Node mode, kernel live migration

## 1. Purpose

CoKernel v1 turns a Windows PC with an NVIDIA GPU into a managed Linux GPU workstation by installing one Windows application.

The user should experience CoKernel as a Windows product. WSL2, Ubuntu, Docker, systemd, Jupyter, MCP, and Secure MCP Tunnel are implementation details hidden behind the application except in diagnostics/developer mode.

Core promise:

> Install CoKernel on Windows, get a self-managed Linux GPU notebook environment that both a human and an AI can use through the same live Jupyter kernel.

v1 is a **fresh product install**, not an in-place upgrade of the experimental v0.1 runtime. v0.1 runtime state is not a compatibility boundary.

## 2. Product boundary

```text
Windows 11
│
├─ CoKernel Desktop UI
│    ├─ dashboard
│    ├─ workspace/runtime controls
│    ├─ tunnel settings
│    ├─ logs/diagnostics
│    └─ update/repair UX
│
├─ CoKernel Host
│    ├─ desired-state controller
│    ├─ WSL lifetime owner
│    ├─ health supervisor
│    ├─ metrics aggregator
│    ├─ secret broker
│    └─ update/repair coordinator
│
└─ WSL2 distro: CoKernel
     ├─ Linux runtime management surface
     ├─ Docker Engine + NVIDIA Container Toolkit
     ├─ WSL loopback bridge
     └─ Compose application stack
          ├─ jupyter
          ├─ mcp
          └─ tunnel
```

### 2.1 Windows is the control plane

Windows owns:

- installation and removal;
- WSL creation/destruction and lifetime;
- desired runtime state;
- automatic start after Windows sign-in;
- health supervision and bounded recovery;
- dashboard data aggregation;
- secret storage;
- update orchestration;
- diagnostics and user notifications.

The Windows UI must not contain the runtime orchestration logic itself. UI and Host are separate conceptual components so the tray/window may exit or restart without destroying the compute runtime.

### 2.2 WSL is the compute plane

WSL owns:

- Linux Python ecosystem;
- CUDA-visible compute execution;
- Docker Engine;
- Jupyter Server/Lab;
- notebook workspace environments;
- Jupyter MCP Server and CoKernel MCP policy extension;
- Secure MCP Tunnel client;
- Linux-side runtime diagnostics.

WSL is intentionally retained because it gives CoKernel a real Linux userspace and therefore a first-class path to Linux-focused frameworks such as JAX while still using the Windows NVIDIA driver GPU virtualization path.

## 3. Core domain concepts

### 3.1 Runtime

A Runtime is the managed WSL + Docker + Jupyter/MCP/Tunnel environment belonging to one Windows installation.

The Runtime has both a **desired state** and an **observed state**.

Desired state:

```text
RUNNING | STOPPED
```

Observed state:

```text
UNINSTALLED
STOPPED
STARTING
HEALTHY
DEGRADED
UPDATING
REPAIRING
STOPPING
ERROR
```

The Host continuously reconciles observed state toward desired state while respecting bounded retry/backoff rules.

### 3.2 Workspace

A Workspace is user-owned notebook/project state.

For v1 the default workspace is a WSL-native directory mounted into the Jupyter container at `/workspace`.

The workspace owns:

- notebooks;
- project source/data selected by the user;
- `pyproject.toml`;
- `uv.lock`;
- `.venv` (reconstructible, not source of truth).

### 3.3 Control-plane Python vs workspace Python

CoKernel deliberately maintains two Python environments.

```text
/opt/cokernel
  CoKernel-owned
  image-built
  Jupyter/control-plane runtime
  not user-mutable during normal operation

/workspace/.venv
  user-owned compute environment
  managed by uv
  notebook default kernel
  pyproject.toml + uv.lock are source of truth
```

These environments must **not** be merged.

This boundary is a v1 architectural invariant and directly resolves Issue #21.

### 3.4 Notebook session

A Notebook Session binds:

- notebook path;
- Jupyter session;
- kernel id;
- workspace;
- live in-memory kernel state.

Human browser and AI must converge on the same Jupyter Server and the same existing kernel for an already-open notebook.

### 3.5 Tool capability

An MCP Tool Capability is a deliberately scoped operation presented to AI clients.

v1 must prefer least-capable tools for common intents rather than forcing benign operations through arbitrary-code or create/modify tools. This is the architectural response to Issue #22.

## 4. Architectural invariants

### INV-001 — WSL is hidden, not removed

The user-facing product is a Windows application; WSL is the managed Linux runtime implementation.

### INV-002 — Dedicated distro

CoKernel uses a dedicated WSL distro. Other user distros are never modified.

### INV-003 — Windows filesystem isolation

Windows drives are not automatically mounted into the CoKernel workload environment. Windows executable interop remains disabled in the managed distro.

### INV-004 — Docker boundary

Jupyter/MCP containers do not receive the Docker socket, Windows home, SSH keys, Git credentials, kubeconfig, browser profiles, or unrelated host secrets.

### INV-005 — Workspace dependency truth

`pyproject.toml` + `uv.lock` define workspace Python dependencies. `uv` is the standard package/environment manager.

### INV-006 — Workspace kernel is default

New Python notebooks use `cokernel-workspace` by default. `sys.executable` in a new notebook must resolve to `/workspace/.venv/bin/python` (or its equivalent resolved path).

### INV-007 — Control-plane environment is immutable at runtime

JupyterLab extensions/language packs are image-owned. The generic Jupyter extension UI must not present a normal-looking mutable PyPI install path that attempts to mutate `/opt/cokernel`.

### INV-008 — Same-kernel attach is fail-closed

Connecting AI to an existing notebook must attach to the unique existing Jupyter session/kernel. Zero or ambiguous matches fail clearly. Connect mode must never silently create another kernel.

### INV-009 — Accurate MCP capability metadata

CoKernel wrappers preserve or improve MCP input schemas and ToolAnnotations. Metadata must describe actual behavior and must never be weakened merely to influence client safety policy.

### INV-010 — Least-capable MCP operations

v1 provides dedicated operations for common narrow intents:

- `connect_notebook` — existing notebook/session only, no file creation, no silent kernel creation;
- `create_notebook` — create-only, no overwrite;
- `list_variables` — bounded safe metadata inspection;
- `get_variable` — one Python identifier, bounded exact built-in serialization only.

Existing upstream compatibility tools may remain exposed, but high-risk arbitrary execution tools stay honestly high-risk.

### INV-011 — Startup dependency order

Runtime start order is logically:

```text
WSL
→ Docker
→ Jupyter
→ Jupyter healthy
→ MCP
→ MCP healthy
→ Tunnel
→ Tunnel ready
→ Windows acceptance
→ HEALTHY
```

Tunnel must not initialize against an MCP listener that has not become healthy.

### INV-012 — Windows owns WSL lifetime

systemd and background containers are not considered sufficient to keep WSL alive. CoKernel Host owns an ordinary persistent WSL client/lifetime mechanism while desired state is RUNNING.

### INV-013 — No public inbound service

Jupyter, MCP diagnostics, runtime management, and tunnel health remain localhost/private-network only. ChatGPT reaches MCP through Secure MCP Tunnel.

### INV-014 — Active kernels are protected from maintenance

A runtime restart/update that interrupts active kernels requires explicit user consent or deferred execution.

### INV-015 — v1 is a clean-install boundary

v0.1 runtime layout, distro contents, `.env`, and Git-checkout-based management are not migrated in place.

The v1 installer detects a legacy CoKernel distro and enters **Legacy Reset** flow. Destruction of the legacy distro is explicit and auditable. Product design does not promise binary/state compatibility with v0.1.

## 5. Legacy Reset and fresh-install policy

The development direction is intentionally clean:

```text
v0.1 experimental runtime
      ↓ explicit reset
DESTROYED
      ↓
CoKernel v1 fresh install
```

### 5.1 What v1 does not migrate

- v0.1 WSL distro system state;
- Docker images/containers;
- v0.1 `.env` secrets;
- v0.1 Git checkout under WSL;
- runtime keeper processes;
- loopback proxy unit instances;
- cached virtual environments.

### 5.2 User data handling

Because notebooks can contain user data, destructive reset is never silent in the released product.

Before unregistering a detected legacy distro, the installer shows:

- legacy distro identity;
- legacy workspace path if discoverable;
- explicit warning that WSL unregister is destructive;
- optional `Export legacy workspace` action;
- explicit confirmation for `Destroy v0.1 and install v1`.

For development/acceptance on the current machine, it is valid to intentionally destroy the current v0.1 distro after any wanted notebook data has been exported.

## 6. Installation model

Final entrypoint:

```text
CoKernelSetup.exe
```

Installer phases:

1. Preflight Windows/virtualization/WSL/NVIDIA/disk checks.
2. Detect existing v1 or legacy v0.1.
3. If legacy: Legacy Reset flow.
4. Enable/update WSL when required; reboot/resume when required.
5. Create fresh dedicated Ubuntu distro.
6. Apply CoKernel isolation policy.
7. Install Docker Engine and NVIDIA Container Toolkit.
8. Install versioned CoKernel runtime payload (not a development Git checkout).
9. Initialize local runtime tokens.
10. Configure loopback bridge.
11. Build/pull runtime images.
12. Validate GPU.
13. Start Host + Runtime.
14. Verify Jupyter/MCP/Tunnel as configured.
15. Mark installation healthy.

## 7. Runtime filesystem model

v1 separates immutable application/runtime payload from mutable state.

Suggested WSL layout:

```text
/opt/cokernel/
  runtime/             versioned product payload
  bin/                 product commands

/etc/cokernel/
  runtime.conf         non-secret system config

/var/lib/cokernel/
  state/               desired/observed metadata
  diagnostics/         bounded diagnostic state

/home/cokernel/
  workspace/           default user workspace
```

The Windows installer package owns the runtime payload. `git pull` is not part of the installed product update path.

## 8. Jupyter UX model

### 8.1 Default kernel

At Jupyter startup, after `uv sync` and kernelspec registration, CoKernel configures the default kernel name to `cokernel-workspace`.

### 8.2 Package installation

Notebook package installation is uv-first:

```bash
uv add numpy
uv add torch
uv add jax
uv remove <package>
uv sync
```

A future v1 UI may wrap these operations as **Workspace Packages**, but the dependency model remains the same.

### 8.3 Jupyter extensions and localization

JupyterLab extension/language-pack installation is a product/image concern, not a notebook dependency concern.

The v1 Jupyter image ships supported extensions at build time. At minimum, the Japanese JupyterLab language pack is built into the image so a Japanese Windows user does not need to use the broken runtime PyPI-manager path observed in v0.1.

Jupyter UI locale is a CoKernel setting. Initial policy:

- default to the Windows UI language when a bundled language pack exists;
- otherwise fall back to English;
- allow explicit language override.

The Jupyter Extension Manager must be disabled or made read-only for runtime mutation.

## 9. MCP capability model

v1 continues to use pinned upstream `jupyter-mcp-server` plus `cokernel-mcp-extension`; no fork is required.

### 9.1 Compatibility layer

Existing upstream tools remain where compatibility is useful. CoKernel wrapper code must preserve exact schema constraints and annotations.

`use_notebook` keeps `Literal["connect", "create"]` and explicit annotations.

### 9.2 CoKernel narrow tools

#### `connect_notebook`

- existing file only;
- existing session/kernel only unless explicit kernel id is supplied;
- no notebook creation;
- no kernel creation;
- idempotent attachment semantics;
- closed-world operation.

#### `create_notebook`

- create-only;
- target must not exist;
- no overwrite;
- kernel creation behavior explicit.

#### `list_variables`

Returns bounded safe metadata only. No arbitrary `repr`, attribute access, iteration of custom objects, or user callable execution.

#### `get_variable`

- accepts one Python identifier only;
- rejects expressions, indexing, calls, imports, comprehensions, operators, attributes;
- serializes only exact approved built-in types;
- bounded depth/item/string/total response size;
- custom/unsupported types return structured metadata without invoking custom representation behavior.

Arbitrary `execute_code` remains a high-risk/open-world capability and is not reclassified.

## 10. Secret model

Windows is the source of truth for user-facing external credentials.

- Tunnel API key is stored with Windows user-scoped secret protection (DPAPI/Credential Manager abstraction).
- MCP/Jupyter local tokens are generated inside the runtime and are not exposed in normal UI.
- Secrets are passed to WSL through a non-command-line secret channel or tightly controlled stdin/file handoff; they are not embedded in process command lines or logs.
- Diagnostic export redacts bearer tokens, API keys, and known secret fields.

## 11. Health and recovery model

Health is layered:

```text
Windows Host
WSL reachable
Docker reachable
Jupyter container healthy
Jupyter HTTP/session API healthy
MCP container healthy
MCP initialize path healthy
Tunnel ready (when configured)
GPU visible in compute container
Workspace writable
```

Recovery escalation:

```text
service restart
→ compose reconcile
→ Docker restart
→ dedicated WSL restart
→ ERROR / user-visible repair action
```

Retries use bounded exponential backoff and a crash-loop circuit breaker.

## 12. Dashboard model

v1 dashboard shows at minimum:

- overall runtime state;
- WSL/Docker/Jupyter/MCP/Tunnel state;
- runtime uptime;
- active kernel count;
- GPU model/utilization;
- VRAM used/total;
- GPU temperature/power when available;
- CPU utilization;
- Windows RAM used/total;
- WSL RAM used/total;
- storage used/total;
- last health error;
- update/repair status.

## 13. Update model

Installed v1 uses product artifacts/releases, not `git pull`.

Update classes:

- Desktop/Host binary;
- WSL runtime payload;
- container images;
- schema/runtime migration.

If active kernels would be interrupted, update is deferred or explicitly confirmed.

v1-to-v1 updates are designed to be convergent and state-preserving. The destructive clean-install rule applies specifically to the v0.1 → v1 product boundary.

## 14. v1 acceptance gates

v1 is not complete until all are true:

1. Fresh `CoKernelSetup.exe` provisions the managed WSL environment.
2. A detected v0.1 environment can be explicitly destroyed and replaced by a fresh v1 environment.
3. Windows sign-in restores desired RUNNING state without manual WSL commands.
4. Jupyter is reachable from Windows and can use the NVIDIA GPU.
5. Linux-only/first-class Python workflows such as JAX installation are viable in the workspace environment.
6. A new notebook defaults to `/workspace/.venv`/`cokernel-workspace`.
7. JupyterLab no longer exposes a broken mutable control-plane PyPI install path.
8. Japanese JupyterLab localization is bundled/configurable rather than installed through the runtime Extension Manager.
9. Human and AI share the exact same live kernel for an existing notebook.
10. MCP annotations/schema are preserved and narrow CoKernel tools from Issue #22 are covered by tests.
11. Dashboard reports CPU/GPU/VRAM/RAM/WSL RAM/storage/service health.
12. Active kernels are protected from unconfirmed disruptive maintenance.
13. Diagnostics can be exported with secrets redacted.
14. The CLI/recovery path remains available for diagnosis even if Desktop UI fails.

## 15. Deferred to post-v1

- second-PC Node mode;
- LAN pairing/mTLS node control;
- remote Jupyter proxy;
- multi-node scheduler;
- batch job queue;
- RTX 5090 + RTX 3080 Fabric resource placement;
- headless boot-before-login node service;
- kernel live migration.

These are designed as extensions of the v1 Host/Runtime boundary, not requirements for the first desktop release.
