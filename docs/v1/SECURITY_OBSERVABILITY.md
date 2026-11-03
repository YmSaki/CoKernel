# CoKernel v1 Security and Observability

Status: **implementation baseline**

## 1. Security objective

CoKernel runs user-controlled Python/AI code on a personal Windows GPU machine. The product must reduce accidental host exposure, isolate product secrets from workload code, and make trust boundaries explicit.

CoKernel does **not** claim malware-grade sandboxing of arbitrary hostile Python code. The managed WSL boundary is a blast-radius reduction mechanism and a Linux execution environment, not a formal hostile-code security sandbox.

The Phase 5 Host bridge uses bounded frames and queues, fresh request IDs and
strict response correlation. It invokes explicit WSL argv with cleared launch
environment, excludes WSLENV/arbitrary credential variables and discards raw
stderr after draining it. Local desired-state files are size/schema validated and
locked to one Host per state directory. This baseline is covered by Host process
tests. The current-user Named Pipe additionally has a protected explicit DACL,
local-only admission, peer token verification and bounded multi-client framing.
Windows import validates an opened regular file and streams bytes through the
Runtime transaction service; it never exposes a Windows path to a worker. These
boundaries have kernel-ACL, malformed-client, cancellation, file-sharing and actual
WSL integration tests. Managed-distro identity, secret-store and tunnel security
remain separate gates. See INTERFACE_CONTRACTS sections 15–18.

Runtime observations are limited independently of persisted notebook results.
Oversized/lagged event streams require an explicit resynchronization; they cannot
silently turn partial output into saved state. Public Session observations omit
worker inspection responses and heartbeat frames. General failure events contain
record identity/classification rather than diagnostic tails. Host tags forwarded
events by bridge generation so a client can reject observations from a replaced
Runtime. See INTERFACE_CONTRACTS section 19.

Package subprocess admission is bounded per Project and globally. Package
specifications cannot inject uv command options; paths are checked against the
managed root at admission and reconciliation. The production Linux uv runner
owns its process group and bounds time/output. Cancellation and bridge EOF reap
uv before releasing the mutation reservation. A saved uncertainty marker prevents
starting a new worker against an unconfirmed environment after Runtime restart.
These controls have dedicated process/RPC tests; they do not establish sandboxing
of malicious package build code. See INTERFACE_CONTRACTS section 20.

Project setup uses the same operation bounds and process ownership. Creation
reserves an exclusive new directory and persists incomplete state before uv starts;
registration resolves a managed path. Cancellation/failure retains user files.
Tests cover duplicate reservations, shared capacity, failed registry admission and
recovery of incomplete initialization after Runtime restart. See section 21.

The shared Runtime endpoint is a trusted in-process adapter API, not an
authentication mechanism. The eventual Unix/MCP transport must authenticate and
select caller origin before submission. Independent clients cannot collide by
reusing request IDs: endpoint delivery restores each caller's ID from a private
slot. Direct calls are frame-bounded and queued/in-flight/unread replies share a
capacity limit. Dropping a reply does not cancel accepted domain work. Tests cover
these limits, ownership checks through the shared path and shutdown cleanup.
Linux bridge input is nonblocking and close-on-exec so shutdown can release a
pending read without inheriting that descriptor into workers. See section 23.

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

The D-028 validation image disables automount, fstab drive mounting, interop and
Windows PATH inheritance before first boot. A root boot command restricts the
distro-local `/run/WSL` to root:root 0700; each managed Runtime launch checks this
boundary. Workloads cannot reach interop servers even by supplying `WSL_INTEROP`,
or change root-owned policy/permissions. Linux root/Windows control remains trusted.

WSL2 registers its PE interpreter at VM scope. An enabled global `WSLInterop`
entry does not prove this distro can execute Windows code. Microsoft documents
that scope and the socket-based execution path in its
[interop architecture](https://github.com/microsoft/WSL/blob/master/doc/docs/technical-documentation/interop.md).
The product must not disable that VM-wide entry and affect unrelated distros.
The root command uses per-distro
[boot configuration](https://learn.microsoft.com/en-us/windows/wsl/wsl-config).
Acceptance must test actual workload execution denial, including after restarting
only the owned distro; configuration text alone is insufficient proof.

## 4. Linux identities and permissions

Target separation:

- product runtime state owned by a runtime identity/root-managed installation;
- Projects and workers run as a non-privileged workload user;
- tunnel secret material readable only by product/tunnel identities that require it;
- runtime Unix sockets use restrictive group/owner permissions;
- Project path permissions prevent accidental cross-Project writes where practical.

The identity/permission launch contract is implemented and validated during Phase
7, including package builds and worker start/stop/cleanup. Phase 9 provisions the
same contract automatically and repeats its checks in the installed managed
distro. Exact account numbers are installation data. Workload code must not
execute as root or silently fall back to the control identity.

Managed launch uses D-027. `COKERNEL_WORKLOAD_UID` and `COKERNEL_WORKLOAD_GID` are
trusted installation inputs, parsed together at Runtime startup, not RPC fields.
Both must be nonzero, distinct from control IDs and not the all-bits-set sentinel.
Runtime validates its non-root UID/GID, exact effective/permitted capability set
(KILL, SETGID, SETUID), zero inheritable capabilities and root-owned 0750 executable
with control primary group. An unprivileged development profile has neither ID
setting and no capabilities; it supplies no managed isolation evidence. Partial
managed configuration or a privileged process without the IDs is rejected.

Projects root and admitted Project directories are setgid 2770, with workload
group and control/workload ownership. Control is a supplementary member of that
group. Runtime and workload creation use umask 0007. Workload code owns its uv
environment, caches and IPython profile; control state remains under separate
0700 control-owned parents. Managed Session sockets have a control-owned 2710
parent with workload group and 0660 socket; workload can traverse/connect but
cannot list or replace endpoints. Peer UID **and** existing PID/descendant checks
must match the launch. The control/MCP Unix API remains 0700/0600 and control-UID
only. Existing arbitrary trees are never recursively chmod/chowned as recovery.

Notebook atomic writes keep temporary content private during writing and use
0660 for completed documents in setgid Project directories. Other directories
retain the existing private file mode. The worker creates its configured IPython
profile as the workload identity before starting IPython, preventing its implicit
fallback to a temporary profile when the parent does not exist.

Before workload exec, supplementary groups are cleared, all three UID/GID values
are changed, all permitted/effective/inheritable/ambient capabilities are cleared,
and no_new_privs is set. Parent-death SIGKILL is armed after credential changes,
with a parent-PID race check. The Runtime retains CAP_KILL for existing owned
group cleanup and sends interrupt/termination directly. This does not expand
ordinary process-group cleanup into a malware-grade escaped-descendant sandbox.

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

All workload launches, including uv package operations/build hooks and worker
startup, use one environment policy. It clears inherited environment, sets cwd
to the explicit Project root and supplies the trusted Runtime PATH, fixed UTF-8
locale, Project-root HOME, Project-local XDG config/cache/data paths and
`.cokernel/ipython` profile, UV_NO_PROGRESS and only explicitly configured
UV_CACHE_DIR/UV_PYTHON_INSTALL_DIR locations. Arbitrary COKERNEL_*, UV_*, PYTHONPATH/PYTHONHOME, loader injection,
tokens and control-home profiles are not inherited. User Project secrets require
explicit Project mechanisms; inherited Runtime environment is not such a mechanism.
The actual MCP supervision smoke injects an unrelated secret sentinel into the
Runtime and checks its absence from both MCP and worker cmdline/environment.
Native Windows development additionally receives SystemRoot and Project-local
USERPROFILE/APPDATA/LOCALAPPDATA/TEMP/TMP. No inherited control profile, proxy,
SSH agent, index credential, UV_CONFIG_FILE, UV_PROJECT_ENVIRONMENT or Python
injection setting is forwarded. Project-owned configuration remains workload data.
This environment boundary does not replace managed Linux identity/file isolation.
In particular, package build hooks are workload code and must eventually run with
the same non-privileged workload identity as notebook workers, never the control
identity. The real build/worker regression checks environment exclusion and
Project home/profile selection; it does not prove filesystem or procfs denial.

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

Resource observations use INTERFACE_CONTRACTS section 28's explicit availability,
source, timestamp and age. Shared collection lives outside request admission;
Windows and Linux snapshots are not summed. The Runtime cgroup limit, Linux-visible
memory total and Windows physical RAM have distinct scopes. Storage is filesystem
capacity rather than recursively scanned Project size. NVIDIA N/A data and unsafe
per-process attribution remain unavailable rather than zero or guessed values.

Collection uses fixed diagnostic arguments/paths, a minimal environment, output
and time limits, and owned subprocess cleanup. It may read OS counters and run a
read-only NVIDIA diagnostic but has no Project, Notebook or Session mutation role.
No metric value or client field selects an executable or a filesystem read path.

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

The Phase 7 Runtime foundation is `runtime.get_diagnostics {}` (schema 1), a
bounded **typed projection** for sharing, not a raw snapshot followed by string
replacement. It includes current Runtime version/time, up to 128 tracked Session
states/generations and their latest failure ID/time/classification/trigger/exit
status/persistence boolean, plus MCP/Tunnel states/generations and up to eight
recent classified process failures each. Session handles may be terminal; archived
on-disk failures are not loaded. Omitted Session counts and excluded categories
are explicit. Unknown service error strings become UNKNOWN, malformed timestamps
become null, and private text is never copied into this schema.

It does not read credentials, environments, Project names/paths, notebook/cell
content, variable values, raw error/stderr/traceback text or diagnostic directories.
There is no credential-registry read just to redact output. Output is capped at
128 KiB; rejection is a fixed diagnostic error, never a partial successful dump.
Document storage stalls do not block this observation. The Runtime advertises
`sanitized-diagnostics`; Host permits forwarding without logging the payload.

This is a Runtime diagnostic primitive, not completion of the full Phase 10 ZIP.
The later bundle must use equivalent explicit projections for Host, resources,
installer/update and historical evidence. Raw FailureRecord files remain local
evidence and can contain workload text; they must never be copied wholesale into
the shareable bundle. Future fields/collectors require the same exclusion tests.

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

### Local MCP foundation verification boundary

Runtime Unix API and MCP bearer files are private to the Linux control identity,
with kernel peer-UID checks and no-follow file validation; exact transport bounds
are defined in INTERFACE_CONTRACTS section 29. A workload identity must differ
from the control identity in the managed installation. Development tests running
Runtime and workers under one UID prove routing, permissions and absence of bearer
inheritance, not denial of credential-file/socket access to hostile worker code.
That cross-identity denial remains mandatory managed-installation acceptance.
The optional development socket and manual MCP command are not installer defaults
or a substitute for supervised product lifecycle and credential provisioning.

Worker launch also sets cwd explicitly to its Project root rather than inheriting
the Runtime control directory. The actual MCP/worker proof checks getcwd against
the Project identity and resolves a relative synchronization file there. This
does not replace managed filesystem identities and access restrictions.
