# CoKernel v1 Implementation Order

Status: **execution baseline**

Implementation should proceed through vertical slices that prove the risky runtime semantics before spending heavily on UI/installer polish.

## Phase 0 — Technical spikes and repository reset

### Deliverables

1. Create new Cargo workspace/layout on `cokernel-v1`.
2. Preserve v0.1 source as reference until replacement paths pass tests; do not make new core depend on it.
3. Spike uv worker-tooling launch strategy:
   - Project interpreter sees Project packages;
   - CoKernel worker/IPython tooling remains product-controlled;
   - Project dependency truth is not unintentionally polluted;
   - selected v1 Python versions work deterministically.
4. Spike IPython embedding/output capture required for:
   - expression result;
   - stdout/stderr;
   - rich MIME display;
   - top-level async;
   - interruption.
5. Spike Tauri/Host Named Pipe integration.
6. Confirm Rust MCP implementation approach/SDK against required Streamable HTTP/tool annotations.

### Gate

No main Session Runtime implementation starts until worker launch and IPython output/interrupt feasibility are demonstrated in small tests.

---

## Phase 1 — Shared domain/protocol core

### Deliverables

Crates:

```text
cokernel-domain
cokernel-protocol
```

Implement:

- IDs/domain structs;
- Runtime/Project/Environment/Notebook/Session/Operation/Failure states;
- structured errors/error codes;
- framed JSON protocol codec;
- version handshake;
- protocol fuzz/malformed-frame/bounds tests.

### Gate

Domain state-machine/unit tests green on Windows and Linux CI.

---

## Phase 2 — Project + uv environment vertical slice

### Deliverables

- `cokernel-runtime` Linux binary skeleton;
- Project registry;
- Project create/open/list;
- uv invocation wrapper;
- package add/remove/sync;
- environment status/generation;
- structured progress/errors;
- Project path safety.

### CLI acceptance

From WSL/dev shell:

```text
cokernel-runtime project create demo
cokernel-runtime package add demo numpy
cokernel-runtime project status demo --json
```

### Gate

Temporary Project can be created/synced and its interpreter imports installed package.

---

## Phase 3 — Python/IPython worker + Session Supervisor

### Deliverables

- `worker/cokernel_worker`;
- private Unix socket worker protocol;
- IPython persistent namespace;
- execution output capture;
- safe variable inspection;
- Session Supervisor;
- FIFO queue;
- start/interrupt/stop/restart;
- failure capture;
- environment-stale state;
- multi-Session concurrency.

### Canonical tests

- Cell 1 `x=123`, Cell 2 `x+1` => 124.
- Session A/B execute concurrently and have separate namespaces.
- force-kill A; B survives; A has FailureRecord.

### Gate

The execution engine works without Desktop/MCP and passes hostile-object safe-inspection tests.

---

## Phase 4 — Notebook Document Service

### Deliverables

- nbformat v4 Rust model preserving unknown JSON metadata;
- notebook create/open/list;
- revisioned edits;
- atomic save;
- external-change detection;
- output conversion from worker events;
- execute-cell by notebook/cell ID;
- Windows-import runtime-side transaction API;
- compatibility fixture suite.

### Gate

A standard imported `.ipynb` can be executed through Project Session, outputs persist, and the result remains structurally valid.

---

## Phase 5 — WSL Runtime bridge + Windows Host

### Deliverables

Crate/binary:

```text
cokernel-host
```

Implement:

- persistent `wsl.exe` bridge;
- protocol handshake;
- desired RUNNING/STOPPED state;
- WSL lifetime ownership;
- reconnection/backoff;
- Windows Named Pipe IPC;
- event forwarding;
- auto-start;
- Windows/WSL metrics aggregation;
- chunked notebook import from Windows.

### Gate

With no Desktop UI, Host can keep Runtime/Sessions alive, execute domain operations through the bridge, restart UI clients freely, and recover bounded bridge failures.

---

## Phase 6 — MCP domain service

### Deliverables

- `cokernel-mcp`;
- local bearer auth;
- Streamable HTTP;
- Runtime Unix API client;
- minimum v1 tool set from `MCP_TUNNEL.md`;
- exact annotations/schema tests;
- response bounds;
- Human/MCP shared queue tests.

### Canonical proof

Human/session executes:

```python
secret_from_master = 123456789
```

MCP `get_variable` on the primary Session returns `123456789` without starting another worker.

### Gate

Same-Session proof and MCP security suite green locally.

---

## Phase 7 — Secure Tunnel integration

### Deliverables

- Windows Secret Store abstraction;
- secure credential handoff;
- Tunnel Supervisor;
- start MCP before tunnel;
- readiness/reconnect/backoff;
- remote access status;
- credential isolation/redaction tests.

### Gate

Remote ChatGPT/MCP reaches the same local Session. Disconnecting tunnel leaves local compute unaffected.

---

## Phase 8 — Desktop application

### Deliverables

Tauri/Desktop:

- Project list/create/open;
- package management;
- notebook tree/editor;
- create/import notebook;
- run cell/run selected/interrupt;
- Session list/restart/stop;
- environment-stale indication;
- CPU/RAM/WSL RAM/GPU/VRAM/storage dashboard;
- crash/failure evidence UI;
- MCP/Remote Access settings/status;
- logs/diagnostics;
- tray and notifications;
- update/repair entry points.

### UX constraints

- no internal PID/session-process selection for normal execution;
- Project implies environment;
- document saved state and Session live state visually distinct;
- visual polish is secondary to understandable operations/state.

### Gate

All representative local use cases can be completed without command-line administration after installation.

---

## Phase 9 — Installer/bootstrap + legacy reset

### Deliverables

- `CoKernelSetup.exe`;
- Windows/WSL/NVIDIA preflight;
- reboot/resume;
- legacy detection/export/explicit destructive reset;
- fresh managed distro;
- identities/permissions/hardening;
- product payload install;
- Host/Desktop install/auto-start;
- first-run GPU/runtime acceptance;
- setup logs.

### Gate

A disposable v0.1 development machine can be explicitly reset and a fresh v1 installed with no manual WSL/Linux configuration.

---

## Phase 10 — Update/repair/product hardening

### Deliverables

- signed/versioned artifact update model;
- Host/Runtime compatibility manifest;
- active-Session disruption analysis;
- defer/confirm update;
- runtime payload migration;
- repair convergence;
- rollback metadata;
- uninstall/data preservation UX;
- crash-loop circuit breakers;
- diagnostic ZIP.

### Gate

v1 build N -> N+1 update preserves Projects/notebooks and never silently terminates active Sessions.

---

## Phase 11 — Full acceptance and release candidate

Execute all `TEST_ACCEPTANCE.md` L0–L6 gates.

Mandatory real-machine evidence includes:

- setup;
- uv Project;
- PyTorch/JAX GPU representative tests where supported;
- notebook import/create/run;
- persistent state;
- parallel Sessions;
- forced worker crash evidence;
- same-Session Human/MCP proof;
- secure tunnel remote proof;
- Windows restart behavior;
- dashboard metrics;
- update guard;
- diagnostic redaction;
- legacy destructive reset once on disposable old runtime.

---

# PR decomposition

Prefer small/medium PRs with one acceptance boundary:

```text
PR A: cargo workspace + domain/protocol
PR B: uv Project manager
PR C: Python worker protocol/IPython
PR D: Session Supervisor
PR E: Notebook service/import
PR F: WSL bridge + Host
PR G: MCP service
PR H: Tunnel/secret integration
PR I: Desktop Project/package shell
PR J: Desktop notebook/session editor
PR K: dashboard/diagnostics
PR L: installer/bootstrap
PR M: update/repair
PR N+: hardening/acceptance fixes
```

Avoid giant PRs combining Runtime, Desktop, Installer, and security changes before their lower-level contracts are proven.

# Implementation discipline

For every phase:

1. implement domain/contract first;
2. add component tests;
3. add failure-path tests;
4. expose through next layer;
5. run acceptance gate;
6. update spec in same PR if behavior/domain contract changes.

No UI workaround should bypass Runtime domain invariants, and no MCP workaround should create a second execution path.