# CoKernel v1 Component Model

Status: **design baseline**  
Depends on: `docs/v1/CONCEPTUAL_DESIGN.md`

## 1. Decomposition policy

CoKernel v1 is divided by ownership boundary, lifecycle, and trust boundary rather than by current script/file layout.

Top-level components:

```text
Windows Control Plane
├─ Desktop UI
├─ Host
├─ Installer/Bootstrapper
├─ Secret Store
├─ Update Manager
└─ Diagnostics/Telemetry

WSL Compute Plane
├─ Runtime Manager
├─ Network Bridge
├─ Docker Runtime
├─ Jupyter Service
│  ├─ Control-plane Python
│  ├─ Workspace Environment Manager
│  └─ Kernel/Session Manager
├─ MCP Service
│  └─ CoKernel MCP Policy Extension
└─ Secure Tunnel Service
```

The v1 implementation may package multiple logical components into one executable initially, but component interfaces must remain explicit so later Node/Fabric work can reuse them.

---

# 2. Windows Control Plane

## C-WIN-01 — CoKernel Desktop UI

**Responsibility**

- render current product state;
- expose start/stop/restart/update/repair actions;
- open Jupyter;
- configure display/runtime preferences;
- configure Secure Tunnel credentials;
- show logs/diagnostics/errors;
- display metrics and active kernel count;
- provide explicit legacy-reset confirmation.

**Must not**

- directly implement WSL lifecycle rules;
- directly parse Docker output as its primary state model;
- hold long-running runtime ownership;
- store plaintext API secrets.

**Inputs**

- Host IPC status stream / request-response API.

**Outputs**

- user intents to Host.

**v1 implementation note**

WPF/.NET 8 is acceptable for v1. WinUI is not an architectural requirement. The UI technology must not leak into runtime contracts.

---

## C-WIN-02 — CoKernel Host

**Responsibility**

This is the central v1 control-plane component.

- persist desired state (`RUNNING|STOPPED`);
- own WSL lifetime while desired state is RUNNING;
- orchestrate runtime startup dependency order;
- poll/aggregate health;
- execute bounded self-healing;
- broker secrets to WSL;
- expose typed IPC to Desktop;
- coordinate update and repair;
- emit structured logs/events;
- publish component state and metrics.

**Runtime state machine**

```text
UNINSTALLED
  ↓ install
STOPPED
  ↓ start
STARTING
  ↓ success
HEALTHY
  ↘ partial failure → DEGRADED
  ↘ fatal/retry exhaustion → ERROR

HEALTHY/DEGRADED
  ↓ update → UPDATING
  ↓ repair → REPAIRING
  ↓ stop → STOPPING → STOPPED
```

**Key principle**

The Host is authoritative for desired/observed state. Docker restart policies and systemd are helpers, not the source of truth.

---

## C-WIN-03 — Host IPC

**Responsibility**

Provide a local-only stable contract between Desktop and Host.

Initial recommendation:

```text
Named Pipe + versioned JSON messages
```

Required operations:

```text
GetStatus
GetMetrics
GetKernels
StartRuntime
StopRuntime
RestartRuntime
UpdateRuntime
RepairRuntime
OpenJupyter
GetLogs
ExportDiagnostics
GetSettings
SetSettings
SetTunnelCredentials
LegacyReset
```

Properties:

- current-user ACL;
- protocol version field;
- request id;
- structured error code;
- cancellation/timeouts;
- no secrets in ordinary status responses.

---

## C-WIN-04 — Installer / Bootstrapper

**Responsibility**

- preflight Windows/WSL/GPU requirements;
- distinguish v0.1 legacy runtime from v1 installation;
- run explicit Legacy Reset;
- create managed WSL distro;
- install versioned runtime payload;
- configure automatic Host startup;
- support reboot-and-resume;
- install/upgrade Desktop + Host;
- establish product version metadata.

**Legacy Reset contract**

A legacy `CoKernel` distro may be destroyed for v1 installation only after explicit user authorization in released builds.

Reset operation:

```text
stop legacy runtime if possible
→ optional workspace export
→ wsl --terminate CoKernel
→ wsl --unregister CoKernel
→ remove legacy Windows runtime state
→ fresh v1 provision
```

No v0.1 WSL image migration is required.

---

## C-WIN-05 — Secret Store

**Responsibility**

Store external credentials and provide a narrow broker API.

Secrets:

- OpenAI tunnel API key;
- future node credentials.

Recommended backing:

- Windows DPAPI/Credential Manager abstraction, current-user scope.

Local Jupyter/MCP runtime tokens remain generated within the runtime unless a Windows-side consumer specifically needs them.

**Rules**

- no API key in command-line arguments;
- no plaintext key in logs;
- no key in diagnostics bundle;
- no key checked into runtime payload.

---

## C-WIN-06 — Metrics Aggregator

**Responsibility**

Aggregate Windows and WSL/compute metrics into one typed snapshot.

Windows sources:

- CPU;
- physical RAM;
- host storage;
- NVIDIA NVML or `nvidia-smi` fallback.

WSL/runtime sources:

- WSL RAM;
- workspace filesystem;
- Docker/runtime state;
- Jupyter kernel count;
- optional per-process/GPU attribution when reliable.

Refresh targets:

- resource metrics: 1–3 s while dashboard visible, slower in tray-only mode;
- health: 2–5 s normal, faster only during startup/recovery.

---

## C-WIN-07 — Health Supervisor

**Responsibility**

Evaluate layered health and trigger bounded recovery.

Health graph:

```text
WSL
└─ Docker
   ├─ Jupyter
   │  ├─ HTTP/API
   │  ├─ workspace writable
   │  └─ GPU visible
   ├─ MCP
   │  └─ initialize/tool discovery
   └─ Tunnel (optional)
      └─ readyz/control-plane connectivity
```

Recovery policy:

```text
Level 1: restart individual service
Level 2: reconcile compose stack
Level 3: restart Docker
Level 4: restart dedicated WSL distro
Level 5: stop automated recovery and surface ERROR
```

Crash-loop protection is mandatory.

---

## C-WIN-08 — Update Manager

**Responsibility**

- discover product update;
- stage signed/versioned artifacts;
- determine whether kernel interruption is required;
- defer or request confirmation;
- apply Windows + WSL runtime update coherently;
- run post-update health acceptance;
- report rollback/recovery state.

**Important change from v0.1**

Installed v1 does not run `git pull` as its product update mechanism.

---

## C-WIN-09 — Diagnostics Manager

**Responsibility**

Create a redacted diagnostics bundle containing:

- product versions;
- Windows/WSL version;
- distro state;
- Docker/runtime info;
- NVIDIA information;
- health snapshot;
- loopback checks;
- Jupyter/MCP/Tunnel status;
- recent structured logs;
- optional smoke-test result.

No secret material may be exported.

---

# 3. WSL Compute Plane

## C-LNX-01 — Runtime Manager

**Responsibility**

Provide a stable Linux-side management surface independent of raw script layout.

v1 target interface can be a single product CLI invoked by Host, e.g.:

```text
cokernel-runtime status --json
cokernel-runtime start
cokernel-runtime stop
cokernel-runtime health --json
cokernel-runtime metrics --json
cokernel-runtime repair
cokernel-runtime logs --json
```

The first implementation may internally call retained shell functions/scripts, but Windows Host must depend on the stable command contract rather than scraping arbitrary shell output.

**Why this exists**

v0.1 has useful shell logic but no stable typed boundary. v1 needs one before Node/Fabric can reuse the runtime remotely.

---

## C-LNX-02 — WSL Bootstrap / Isolation

**Responsibility**

- install Linux prerequisites;
- Docker Engine;
- NVIDIA Container Toolkit;
- enable systemd;
- set managed distro marker/version;
- disable Windows drive automount;
- disable Windows executable interop;
- configure users/groups/permissions.

Bootstrap is privileged and separate from ordinary runtime control.

---

## C-LNX-03 — Network Bridge

**Responsibility**

Expose real WSL localhost listeners for Windows localhost forwarding while keeping Docker backend ports private.

Logical mapping:

```text
Windows localhost:8888
  → WSL listener:8888
  → Docker backend:18888
  → jupyter:8888

Windows localhost:4040
  → WSL listener:4040
  → Docker backend:14040
  → mcp:4040

Windows localhost:8080
  → WSL listener:8080
  → Docker backend:18080
  → tunnel:8080
```

The current `systemd-socket-proxyd` approach is retained unless a simpler equally secure mechanism is proven on real machines.

---

## C-LNX-04 — Container Orchestrator

**Responsibility**

Manage the Compose application stack.

Services:

- `jupyter`;
- `mcp`;
- optional `tunnel`.

Rules:

- backend ports bind to WSL loopback only where publishing is needed;
- Jupyter receives GPU;
- MCP does not require GPU;
- workload containers do not receive Docker socket;
- tunnel starts only after MCP healthy;
- healthchecks are authoritative inputs, not the global desired-state controller.

---

# 4. Jupyter service internal components

## C-JUP-01 — Control-plane Python Runtime

Path:

```text
/opt/cokernel
```

Owns:

- JupyterLab;
- jupyter-collaboration;
- infrastructure extensions;
- product-selected JupyterLab language packs/extensions.

Policy:

- immutable during normal operation;
- built by image;
- no runtime `pip install` UX;
- not the notebook dependency environment.

---

## C-JUP-02 — Workspace Environment Manager

Path:

```text
/workspace
/workspace/.venv
```

Owns:

- creation of minimal workspace project when absent;
- `uv sync`;
- dependency state;
- kernelspec registration;
- future `Workspace Packages` actions.

Source of truth:

```text
/workspace/pyproject.toml
/workspace/uv.lock
```

Operations eventually exposed through Host/UI:

```text
ListPackages
AddPackage
RemovePackage
SyncEnvironment
EnvironmentStatus
```

These are uv operations, not mutations of `/opt/cokernel`.

---

## C-JUP-03 — Kernel/Session Manager

**Responsibility**

- register `cokernel-workspace`;
- configure it as default Python kernel;
- enumerate sessions/kernels;
- support active-kernel protection during update;
- provide the same-kernel lookup primitive used by MCP.

Invariant:

New notebooks must not accidentally run against `/opt/cokernel/bin/python`.

---

## C-JUP-04 — Jupyter UX Configuration

**Responsibility**

- default kernel selection;
- JupyterLab locale;
- disable/read-only Extension Manager mutation path;
- product branding/integration where useful;
- supported language packs installed at image build.

Japanese localization is a first-class v1 configuration, not a runtime PyPI-install workaround.

---

# 5. MCP service internal components

## C-MCP-01 — Upstream Jupyter MCP Server

Pinned upstream dependency remains the base implementation.

CoKernel does not fork it in v1 unless an extension API limitation is proven.

---

## C-MCP-02 — Same-Kernel Resolver

Existing CoKernel concept retained.

Input:

- notebook path;
- optional explicit kernel id.

Output:

- unique existing kernel id or explicit failure.

No silent kernel creation when attaching to an existing notebook.

---

## C-MCP-03 — Tool Metadata Adapter

**Responsibility**

When wrapping/replacing upstream tools:

- preserve exact annotations;
- preserve precise input types such as `Literal`;
- regression-test tool metadata;
- never accidentally fall back to conservative/wrong default annotations.

This component fixes the metadata regression identified by Issue #22.

---

## C-MCP-04 — Narrow Capability Tools

v1 CoKernel-specific tools:

### `connect_notebook`

Existing notebook/session only.

### `create_notebook`

Create-only/no-overwrite.

### `list_variables`

Bounded safe variable metadata.

### `get_variable`

Identifier-only, bounded exact-type serialization.

These tools reduce the need for broad `use_notebook`/`execute_code` capabilities for benign intents.

---

## C-MCP-05 — Transport Security Adapter

Retains DNS-rebinding protection while allowlisting the private Compose service hostname required by tunnel-client.

No global Host-validation disable.

---

# 6. Secure Tunnel

## C-TUN-01 — Tunnel Client

OpenAI Secure MCP Tunnel remains the remote transport from ChatGPT to private MCP.

CoKernel responsibilities around it:

- inject private MCP bearer auth internally;
- start only after MCP is healthy;
- use `readyz` as runtime readiness;
- no OAuth requirement for CoKernel's MCP auth model;
- preserve admin UI loopback restrictions;
- keep OpenAI control-plane access outbound only.

---

# 7. Cross-component contracts

## 7.1 Status contract

A typed status snapshot contains at least:

```text
product_version
runtime_version
desired_state
observed_state
wsl
container_runtime
jupyter
mcp
tunnel
workspace
gpu
active_kernel_count
last_error
last_transition_at
```

Each component status has:

```text
state
healthy
message
error_code?
last_checked_at
```

## 7.2 Error contract

Errors exposed to Desktop must include:

```text
code
component
summary
action
technical_detail?
```

Example:

```text
CK-MCP-004
MCP
"MCP did not become ready"
"Restart MCP or run Diagnostics"
```

Raw exit codes are diagnostic detail, not the primary user message.

## 7.3 Logging contract

Structured event fields:

```text
timestamp
severity
component
event_id
operation_id?
message
error_code?
```

Secrets are redacted at source where possible and again during bundle export.

---

# 8. Component ownership summary

| Component | Runs on | Product-owned state | User-owned state |
|---|---|---|---|
| Desktop UI | Windows | preferences/UI cache | none |
| Host | Windows | desired state, health metadata | none |
| Secret Store | Windows | encrypted credentials | credentials supplied by user |
| Installer | Windows | installed product files | none |
| Runtime Manager | WSL | runtime config/state | none |
| Network Bridge | WSL | systemd units/config | none |
| Docker Runtime | WSL | images/containers | none |
| Jupyter control plane | container | `/opt/cokernel` | none |
| Workspace Env Manager | container/workspace | generated metadata | `pyproject.toml`, `uv.lock`, notebooks |
| MCP | container | tool/session attachment state | none |
| Tunnel | container | ephemeral runtime state | none |

---

# 9. Dependency direction

Allowed dependency direction:

```text
Desktop UI
   ↓
Host API
   ↓
Runtime Management Contract
   ↓
WSL/Docker services
   ↓
Jupyter/MCP/Tunnel
```

MCP may call Jupyter APIs. Tunnel may call MCP. Jupyter must not depend on Desktop UI. Runtime containers must not depend on Windows source checkout structure.

This direction is mandatory for later CoKernel Node/Fabric reuse.
