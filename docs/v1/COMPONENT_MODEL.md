# CoKernel v1 Component Model

Status: **implementation baseline**  
Depends on: `REQUIREMENTS.md`, `DOMAIN_MODEL.md`, `ARCHITECTURE.md`

CoKernel v1 is decomposed by ownership, lifecycle, trust boundary, and failure containment.

## 1. Top-level component map

```text
Windows
├─ C-WIN-01 Desktop UI
├─ C-WIN-02 Host
├─ C-WIN-03 Host IPC Server
├─ C-WIN-04 Secret Store
├─ C-WIN-05 Update/Repair Manager
└─ C-WIN-06 Bootstrap/Installer

Managed WSL
├─ C-LNX-01 Runtime Bridge/Daemon
├─ C-LNX-02 Project Registry
├─ C-LNX-03 uv Environment Manager
├─ C-LNX-04 Notebook Document Service
├─ C-LNX-05 Session Supervisor
├─ C-LNX-06 Metrics/Diagnostics Collector
├─ C-LNX-07 MCP Service
└─ C-LNX-08 Tunnel Supervisor

Per live Session
└─ C-EXE-01 Python/IPython Worker
```

## 2. C-WIN-01 — Desktop UI

### Responsibilities

- Project list/create/open/delete actions;
- package add/remove/sync UX;
- notebook create/import/open/edit/save UX;
- cell execution/interrupt/restart/stop controls;
- Session/resource state display;
- MCP/Remote Access settings/status;
- logs/diagnostics/update/repair UX;
- file picker and Windows-side notebook import initiation;
- tray presence and notifications.

### Must not

- directly own WSL lifetime;
- directly invoke uv as the authoritative environment path;
- directly spawn Session workers;
- store plaintext tunnel secrets;
- write Project `.ipynb` files behind the Runtime document service.

### Dependency

Desktop -> current-user Host IPC only.

## 3. C-WIN-02 — Host

Long-running user-level Windows control process.

### Responsibilities

- persist desired runtime state;
- own the long-lived `wsl.exe` bridge while RUNNING;
- reconnect/restart the bridge with bounded backoff;
- broker Desktop requests to Runtime;
- aggregate Windows + Runtime metrics;
- manage Windows auto-start;
- hold/broker external secrets;
- coordinate installer/update/repair operations;
- emit user notifications;
- preserve runtime operation IDs across UI reconnects where practical.

### Failure property

Desktop failure must not imply Runtime/Session termination. Host failure may drop the Windows bridge, but Runtime behavior/recovery must be explicit and diagnosable.

## 4. C-WIN-03 — Host IPC Server

### Transport

Windows Named Pipe, current-user ACL.

### Responsibilities

- versioned request/response methods;
- asynchronous event subscription;
- request IDs and operation IDs;
- cancellation/timeouts;
- structured errors;
- streaming/chunked notebook import payloads;
- no secret echo in status/event responses.

### Example methods

```text
runtime.status
runtime.start
runtime.stop
projects.list
projects.create
projects.open
packages.list
packages.add
packages.remove
packages.sync
notebooks.list
notebooks.create
notebooks.import
notebooks.get
notebooks.apply_edits
sessions.list
sessions.start
sessions.execute_cell
sessions.interrupt
sessions.restart
sessions.stop
resources.get
remote_access.status
remote_access.configure
diagnostics.export
```

## 5. C-WIN-04 — Secret Store

### Backing

Windows user-scoped DPAPI/Credential Manager abstraction.

### Owns

- OpenAI/Secure Tunnel external credentials;
- future remote-node credentials.

### Does not own

- user Project secrets placed intentionally in Project files;
- volatile local runtime/session IDs.

### Rules

- no secrets in command-line arguments;
- no secrets in ordinary logs/events;
- no secrets copied into Project directories;
- diagnostics redact known secret forms.

## 6. C-WIN-05 — Update/Repair Manager

### Responsibilities

- discover/stage versioned product artifacts;
- determine disruptive/non-disruptive update impact;
- protect active Sessions from surprise termination;
- coordinate Windows binaries and WSL runtime payload updates;
- run post-update acceptance;
- initiate repair/convergence;
- retain rollback metadata where supported.

## 7. C-WIN-06 — Bootstrap/Installer

### Responsibilities

- prerequisite/preflight checks;
- WSL enable/update/reboot-resume;
- legacy-runtime detection and explicit reset flow;
- dedicated distro provisioning;
- Linux identities/permissions;
- runtime payload installation;
- uv and GPU prerequisites;
- Host/Desktop installation and auto-start registration;
- first-run acceptance.

---

## 8. C-LNX-01 — Runtime Bridge/Daemon

Central Linux control-plane process.

### Interfaces

- framed stdio bridge to Windows Host;
- Unix domain socket API for local MCP and other trusted product services.

### Responsibilities

- protocol handshake/versioning;
- route domain operations;
- Project registry;
- Session lifecycle ownership via Session Supervisor;
- Runtime health state;
- child service supervision for MCP/Tunnel;
- Linux-side structured logging;
- expose metrics/diagnostic operations.

### Must not

- execute notebook user code in-process;
- expose tunnel secrets to workers;
- let one client bypass domain invariants.

## 9. C-LNX-02 — Project Registry

### Responsibilities

- assign/stabilize `project_id`;
- map Project ID to canonical WSL-native root;
- validate Project root containment/permissions;
- discover notebooks;
- track recent/open Projects;
- provide Project metadata independent of live environment state.

### Persistence

Small product-owned registry under `/var/lib/cokernel/state/` plus Project-local files where appropriate.

## 10. C-LNX-03 — uv Environment Manager

### Responsibilities

- initialize uv Project when requested;
- detect Python/dependency requirements;
- create/sync environment;
- add/remove packages;
- query dependency state;
- expose uv operation progress/errors;
- compute environment generation/fingerprint;
- mark existing Sessions stale after successful environment mutation;
- manage/cache cleanup commands without breaking active Projects.

### Source of truth

`pyproject.toml` + `uv.lock`.

### Must not

- silently restart Sessions after environment mutation;
- replace uv dependency resolution with product-specific resolution.

## 11. C-LNX-04 — Notebook Document Service

### Responsibilities

- parse/validate nbformat v4 `.ipynb` JSON;
- preserve unknown metadata;
- create/import/open documents;
- stable notebook/cell IDs;
- document revision control;
- apply human/AI edits;
- apply execution outputs;
- atomic file writes;
- external modification detection;
- emit notebook revision events;
- normalize only what is necessary for valid output.

### Ownership rule

Only this service writes open NotebookDocuments as part of normal CoKernel operation. Workers emit results; they do not persist notebook files.

## 12. C-LNX-05 — Session Supervisor

### Responsibilities

- one primary live Session per notebook by default;
- spawn worker with Project interpreter/environment;
- maintain Session state machine;
- per-Session FIFO execution queue;
- allow different Sessions to run concurrently;
- interrupt/stop/restart workers;
- capture child PID/exit status/stdout/stderr;
- correlate failures with OOM/resource/runtime evidence;
- retain bounded FailureRecords;
- publish state/execution events;
- mark environment-stale Sessions after Project sync.

### Critical failure boundary

A worker crash must not crash the Runtime daemon or unrelated workers.

## 13. C-LNX-06 — Metrics/Diagnostics Collector

### Sources

- `/proc` and process metadata;
- WSL memory state;
- filesystem usage;
- NVIDIA management interface / `nvidia-smi` fallback;
- child process resource data;
- Runtime/MCP/Tunnel supervision state;
- kernel/OS OOM evidence where available.

### Responsibilities

- typed resource snapshots;
- best-effort per-Session attribution;
- diagnostic bundle source data;
- bounded log collection;
- redaction support.

## 14. C-LNX-07 — MCP Service

Separate product process/service using Runtime domain API.

### Responsibilities

- Streamable HTTP MCP endpoint;
- local bearer authentication;
- domain-oriented tools;
- exact tool annotations/schema;
- input validation/bounds;
- map all stateful operations to Runtime IDs/queues;
- never create hidden execution state outside Session Supervisor.

### Must not

- directly spawn Python workers;
- directly edit `.ipynb` behind Notebook Document Service;
- store external tunnel credentials.

## 15. C-LNX-08 — Tunnel Supervisor

### Responsibilities

- start tunnel only after MCP ready;
- receive credential material through controlled handoff;
- monitor readiness/reconnect;
- expose no secret material to notebook workers;
- keep tunnel failure independent from local compute;
- stop tunnel when Remote Access disabled.

---

## 16. C-EXE-01 — Python/IPython Worker

One worker per ExecutionSession.

### Responsibilities

- connect to supervisor private Unix socket;
- start IPython execution shell;
- maintain user namespace;
- execute queued cell source;
- capture/emit ordered output events;
- top-level async/IPython behavior;
- constrained variable inspection;
- cooperative interrupt handling;
- heartbeat/status response.

### Must not

- manage uv dependency state;
- write notebook files;
- contact MCP/Tunnel directly;
- receive Windows/tunnel/MCP credentials;
- supervise other workers.

## 17. Dependency direction

Allowed normal direction:

```text
Desktop
  -> Host IPC
     -> Host
        -> WSL Runtime bridge
           -> Runtime domain services
              -> Project/uv/Notebook/Session services
                 -> Worker

AI
  -> Tunnel
     -> MCP
        -> Runtime domain services
```

Forbidden shortcuts include Desktop -> worker, MCP -> worker direct spawn, and worker -> notebook file persistence.

## 18. Event flow example — Execute Cell

```text
Desktop/MCP
  -> sessions.execute_cell(session_id, notebook_id, cell_id, revision)
Host (Desktop path only)
  -> Runtime
Session Supervisor
  -> enqueue operation
  -> Worker execute request
Worker
  -> stdout/display/result/error events
Supervisor
  -> operation events
Notebook Document Service
  -> apply final/stream-supported outputs with revision control
Runtime
  -> subscribers
Desktop/MCP caller
```

## 19. Event flow example — Worker crash

```text
Worker exits
  -> Supervisor records exit/signal/stderr
  -> Metrics Collector snapshots RAM/GPU/OOM context
  -> FailureRecord persisted
  -> Session = CRASHED
  -> Runtime event published
  -> Desktop/AI sees failure evidence
  -> restart only on explicit/defined recovery action
```

## 20. Component testability rule

Every component must have a test seam that does not require the complete product:

- uv manager can run against temporary Projects;
- notebook service can run against fixture `.ipynb` files;
- worker protocol can run with fake supervisor;
- supervisor can run with crash/slow/fake workers;
- MCP can run against a fake Runtime domain service;
- Host can run against a fake WSL bridge;
- Desktop can run against a fake Host IPC server.