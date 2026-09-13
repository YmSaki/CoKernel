# CoKernel v1 Security and Observability

Status: **implementation baseline**

## 1. Security objective

CoKernel runs user-controlled Python/AI code on a personal Windows GPU machine. The product must reduce accidental host exposure, isolate product secrets from workload code, and make trust boundaries explicit.

CoKernel does **not** claim malware-grade sandboxing of arbitrary hostile Python code. The managed WSL boundary is a blast-radius reduction mechanism and a Linux execution environment, not a formal hostile-code security sandbox.

## 2. Trust zones

```text
Zone A — Windows user/control plane
  Desktop, Host, Windows Secret Store

Zone B — WSL product control plane
  Runtime, MCP, Tunnel Supervisor, product config/state

Zone C — Project/workload execution
  Project files, uv environment, Python/IPython workers

Zone D — External AI/network
  Secure Tunnel control plane, MCP clients
```

Normal trust direction is tightly mediated between zones.

## 3. WSL boundary

Requirements:

- dedicated CoKernel distro;
- Windows drive automount disabled for managed workload runtime;
- Windows executable interop disabled;
- user code should not transparently inherit Windows home/credential paths;
- GPU virtualization remains enabled;
- product/bootstrap may use privileged Linux operations, workers do not.

## 4. Linux identities and permissions

Target separation:

- product runtime state owned by a runtime identity/root-managed installation;
- Projects and workers run as a non-privileged workload user;
- tunnel secret material readable only by product/tunnel identities that require it;
- runtime Unix sockets use restrictive group/owner permissions;
- Project path permissions prevent accidental cross-Project writes where practical.

Exact Linux account layout is validated during installer implementation, but workload code must not execute as root.

## 5. Secret classes

### Windows external credentials

Examples: Secure Tunnel/API credential.

Source of truth: user-scoped Windows secret protection.

### Local runtime credentials

Examples: MCP bearer/token.

Generated and owned by product runtime; not exposed as normal user configuration.

### Project/user secrets

If the user intentionally stores API keys or `.env` files in a Project, those belong to Project/workload trust and are outside CoKernel's external credential isolation guarantee.

## 6. Secret prohibitions

External/product secrets must not appear in:

- Project files;
- notebook metadata/outputs;
- worker environment unless explicitly a user Project secret;
- worker command lines;
- normal logs;
- error messages;
- diagnostic ZIPs;
- MCP tool output.

Redaction is defense-in-depth; secrets should be absent at source whenever possible.

## 7. Notebook worker security posture

Worker code can:

- read/write its permitted Project files;
- import Project dependencies;
- use CPU/RAM/GPU;
- make network requests according to ordinary Linux/network policy;
- spawn child processes unless restricted later.

Worker code cannot rely on access to:

- Windows mounted drives;
- Windows Credential Manager/DPAPI;
- external tunnel key;
- local MCP bearer;
- privileged Runtime state;
- Host IPC directly.

## 8. MCP authorization posture

v1 assumes a single local Windows user/product owner rather than multi-tenant RBAC.

Security controls still include:

- local bearer authentication;
- private/local listener;
- precise tool capabilities;
- input/path/domain-ID validation;
- no implicit privilege escalation from read-only tools;
- tunnel-controlled remote route.

Future multi-user/Fabric work will require a richer identity/authorization model and must not assume the v1 single-user model scales unchanged.

## 9. Path safety

All Project-relative file operations must:

- canonicalize/normalize paths;
- reject traversal outside Project root;
- handle symlinks deliberately rather than assuming lexical prefix is sufficient;
- reject writes through links that escape permitted roots;
- use atomic temp files within the destination filesystem;
- define behavior for Windows-import filenames invalid on Linux or vice versa.

Import operations never execute notebook content.

## 10. Supply-chain/update security

Release product should support:

- signed Windows binaries/installer;
- versioned runtime payloads;
- checksum/signature verification before update install;
- pinned/locked product dependencies in build/release pipelines;
- provenance/build metadata retained for diagnostics.

Project Python dependencies remain user-selected and are not security-vetted by CoKernel merely because uv installs them.

## 11. Observability objectives

The user should be able to answer:

- Is CoKernel healthy?
- Which Projects/Sessions are running?
- What is CPU/RAM/GPU/VRAM usage?
- Is remote AI access connected?
- What operation is currently executing?
- Why did a Session stop/crash, insofar as evidence permits?
- What can I do next?

## 12. Structured logging

Product logs are structured JSONL internally, renderable as readable text in UI.

Required fields:

```text
timestamp
severity
component
event_id
message
runtime_id?
project_id?
notebook_id?
session_id?
operation_id?
error_code?
evidence?
```

Severities:

```text
TRACE
DEBUG
INFO
WARN
ERROR
FATAL
```

Debug/trace logs are bounded/rotated and not enabled unbounded by default.

## 13. Component event IDs

Use stable event families, for example:

```text
HOST.BRIDGE.START
HOST.BRIDGE.EXIT
RUNTIME.START
PROJECT.CREATE
UV.SYNC.START
UV.SYNC.FAIL
NOTEBOOK.IMPORT
NOTEBOOK.CONFLICT
SESSION.START
SESSION.EXECUTE
SESSION.INTERRUPT
SESSION.CRASH
SESSION.STALE_ENV
MCP.START
MCP.CALL
TUNNEL.CONNECTED
TUNNEL.RETRY
UPDATE.START
```

This makes support/automated diagnostics independent of prose wording.

## 14. User-facing error model

Every actionable error contains:

```text
code
component
summary
what_is_known
action
technical_detail? (expandable)
```

Example:

```text
CK-SES-004
Execution Session
"Python process exited unexpectedly"
"The process was killed with SIGKILL. An OOM event was detected near the same time."
"Restart the Session or reduce memory use."
```

If root cause cannot be determined:

```text
"Cause could not be determined from available evidence."
```

CoKernel must not turn correlation into certainty.

## 15. Metrics

### Host/machine

- CPU utilization;
- host physical memory used/total;
- relevant host/storage usage.

### WSL/runtime

- WSL memory used/limit/total context;
- Project/storage usage where available;
- Runtime/MCP/Tunnel process state.

### GPU

- GPU name/id;
- utilization;
- VRAM used/total;
- temperature when available;
- power when available;
- process-level VRAM attribution where reliable.

### Session

- state;
- worker PID;
- uptime;
- queue depth;
- current operation;
- RSS/CPU when available;
- GPU attribution when available.

## 16. Metrics cadence

Target behavior:

- dashboard-visible resources: roughly 1–3 second cadence;
- tray/background resources: slower cadence to reduce overhead;
- startup/health transitions: event-driven plus short polling where required;
- resource sampling must not materially perturb GPU/CPU workloads.

Exact intervals are tuneable settings/constants, not protocol invariants.

## 17. Failure evidence capture

On worker crash, capture before cleanup where possible:

- process exit status/signal;
- worker startup/environment generation;
- current operation/cell;
- bounded stdout/stderr tail;
- recent Session event tail;
- WSL memory snapshot;
- host memory snapshot;
- GPU snapshot;
- OOM/journal evidence accessible to runtime;
- whether Runtime/WSL restarted concurrently.

Evidence is stored as a bounded `FailureRecord` and linked from Session state.

## 18. Diagnostic bundle

User can export a ZIP containing redacted:

- product/runtime versions/build IDs;
- Windows version;
- WSL version/distro state;
- GPU/driver information;
- uv version/cache summary;
- Project registry metadata (paths may be normalized/redacted according to policy);
- Runtime/MCP/Tunnel health;
- recent product logs;
- Session FailureRecords;
- network/tunnel readiness diagnostics;
- installation/update logs;
- acceptance/smoke check results.

Do not include notebook/project file contents by default. A user can explicitly attach a notebook separately if needed.

## 19. Crash-loop protection

For Host bridge, Runtime child services, MCP, and Tunnel:

```text
retry with bounded exponential backoff
-> detect repeated failure window
-> stop automatic restart after threshold
-> surface ERROR/DEGRADED + repair action
```

Session workers are different: an unexpected Session crash is normally left `CRASHED` rather than endlessly auto-restarted, because automatic restart loses in-memory state and can hide the actual failure.

## 20. Health model

Layered health:

```text
Windows Host
  -> WSL bridge
     -> Runtime
        -> Project Environment service
        -> Session Supervisor
        -> GPU availability
        -> MCP (optional for local compute)
        -> Tunnel (optional for local compute)
```

Overall runtime can be `HEALTHY` while Remote Access is `DEGRADED` if local compute remains fully usable. UI should show component health instead of collapsing every optional-service problem into total failure.

## 21. Security/observability acceptance tests

- worker cannot see tunnel/MCP/Windows external credentials;
- Windows drives absent from worker's normal filesystem view;
- path traversal/symlink escape tests;
- MCP auth required;
- diagnostics secret-redaction corpus;
- forced Session SIGKILL creates FailureRecord;
- simulated OOM evidence yields `suspected` classification correctly;
- unknown crash remains unknown rather than fabricated;
- MCP/tunnel crash does not terminate worker Session;
- log rotation/bounds under stdout spam;
- UI error contains code, scope, evidence, next action.