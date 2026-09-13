# CoKernel v1 Domain Model

Status: **implementation baseline**

The domain model exists to keep product concepts independent from implementation plumbing. User-facing features, MCP, UI, and tests should speak in these terms.

## 1. MachineRuntime

Represents the managed CoKernel installation on one Windows PC.

Key fields:

```text
runtime_id
product_version
runtime_version
desired_state
observed_state
wsl_state
gpu_inventory
resource_snapshot
last_error
```

Relationships:

```text
MachineRuntime
  1 -> many Projects
  1 -> many ExecutionSessions
  1 -> one MCP service
  1 -> zero/one SecureTunnel connection
```

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

## 2. Project

A Project is the primary unit of work and the boundary for Python dependency state.

Key fields:

```text
project_id
name
root_path
python_requirement
environment_state
created_at
last_opened_at
```

Owned state:

```text
pyproject.toml
uv.lock
source/data/notebook files
project metadata
```

Derived/reconstructible state:

```text
virtual environment
uv resolver/install artifacts
runtime package indexes/cache links
```

Relationships:

```text
Project
  1 -> 1 ProjectEnvironment
  1 -> many NotebookDocuments
  1 -> many ExecutionSessions over time
```

Invariant: Project environment selection is implicit when executing a Project notebook.

## 3. ProjectEnvironment

Represents the runnable Python environment derived from Project dependency state.

Key fields:

```text
project_id
python_executable
python_version
lock_fingerprint
environment_generation
sync_state
last_sync_at
```

States:

```text
ABSENT
SYNCING
READY
BROKEN
```

`environment_generation` increments after a successful dependency/environment mutation that may change what a newly started Python process sees.

A Session records the environment generation it started with. This allows CoKernel to mark old Sessions as environment-stale without destroying them.

## 4. NotebookDocument

Represents one `.ipynb` document within a Project.

Key fields:

```text
notebook_id
project_id
relative_path
format_major
format_minor
revision
content_hash
modified_at
```

A NotebookDocument contains ordered cells and notebook metadata.

Important invariant:

```text
NotebookDocument != ExecutionSession
```

Saving the document does not persist arbitrary live Python objects. Losing a Session does not delete the notebook document.

### Notebook cell

Key fields:

```text
cell_id
cell_type: code | markdown | raw
source
metadata
execution_count?
outputs[]
```

Cell IDs are preserved when valid. CoKernel generates stable IDs for new cells.

## 5. ExecutionSession

Represents one persistent supervised Python/IPython execution process.

Key fields:

```text
session_id
project_id
notebook_id
worker_pid?
worker_generation
environment_generation
state
started_at
last_activity_at
current_operation_id?
queue_depth
failure_record_id?
```

States:

```text
STARTING
IDLE
EXECUTING
INTERRUPTING
STALE_ENVIRONMENT
STOPPING
STOPPED
CRASHED
ERROR
```

### Primary Session rule

For v1, each notebook has at most one **primary live Session** by default.

This gives Human UI and AI one deterministic shared namespace.

The architecture may support additional explicit Sessions later without changing the NotebookDocument model.

### Session namespace

The Python namespace is process-local volatile state.

Examples:

```text
variables
imports
loaded models
open handles
framework runtime state
GPU allocations
```

It is not treated as document persistence.

## 6. ExecutionOperation

Represents one queued/running execution action inside a Session.

Key fields:

```text
operation_id
session_id
origin: HUMAN | MCP | INTERNAL
kind: EXECUTE_CELL | EXECUTE_CODE_INTERNAL | INTERRUPT
cell_id?
requested_at
started_at?
finished_at?
status
```

Status:

```text
QUEUED
RUNNING
SUCCEEDED
FAILED
INTERRUPTED
CANCELLED
```

Invariant: one Session executes at most one user-code operation at a time.

Different Sessions may execute operations concurrently.

## 7. ExecutionResult

Represents the observable result of one operation.

Contains zero or more ordered events:

```text
stdout
stderr
execute_result
rich_display
error
status/progress
```

A rich display uses MIME-keyed data compatible with `.ipynb` output semantics, e.g.:

```text
text/plain
text/html
image/png
image/svg+xml
application/json
```

The result also contains timing and final status metadata.

## 8. FailureRecord

Immutable diagnostic record captured when a Session or critical runtime component fails.

Key fields:

```text
failure_id
component
session_id?
operation_id?
cell_id?
timestamp
exit_code?
signal?
last_stderr
worker_pid?
worker_generation?
worker_started_at?
environment_generation?
worker_memory_snapshot
linux_oom_evidence
wsl_memory_snapshot
host_memory_snapshot
gpu_snapshot
runtime_event_context
classification
confidence
```

`runtime_event_context` records the structured failure trigger and the bounded pre-failure Session evidence tail. The Session tail contains lifecycle/operation/worker-event metadata only; it does not duplicate stdout/stderr/rich-output payloads. Runtime-local evidence is captured before cleanup where possible. Host memory, GPU, and Runtime/WSL restart correlation are attached by the Windows Host/metrics boundary when that evidence source is available; absence of those sources must remain explicit rather than being inferred.

Linux OOM classification uses per-worker cgroup-v2 `memory.events` baselines captured at Session worker start. `OOM_SUSPECTED` is only produced from a same-cgroup positive `oom_kill` delta associated with an already-terminated worker. Cumulative counters, cgroup changes, counter resets, or Supervisor-generated recovery termination do not establish OOM causality.

`classification` may be:

```text
OOM_SUSPECTED
PROCESS_SIGNAL
PYTHON_EXCEPTION
CUDA_OR_NATIVE_FAILURE_SUSPECTED
RUNTIME_RESTART
MANUAL_TERMINATION
UNKNOWN
```

Classification is evidence-based; CoKernel must not fabricate a root cause.

## 9. PackageDependency

Represents Project dependency information as exposed to users/API.

Key fields:

```text
name
requested_constraint
resolved_version?
source?
```

The source of truth remains Project dependency files managed by uv; this object is an API/UI projection.

## 10. ResourceSnapshot

Represents metrics at a point in time.

Machine fields:

```text
cpu_percent
host_memory_used/total
wsl_memory_used/total
storage_used/total
gpus[]
```

GPU fields:

```text
gpu_id
name
utilization
vram_used/total
temperature?
power?
```

Optional Session attribution:

```text
session_id
pid
cpu
rss
gpu_process_memory?
```

Attribution is best-effort and must be identified as unavailable when the platform cannot reliably provide it.

## 11. MCPClientConnection

Represents an authenticated AI-facing connection to the CoKernel MCP service.

The connection does not own a Session. It references Project/Notebook/Session objects through domain IDs.

This prevents AI attachment from implicitly creating separate compute state.

## 12. SecureTunnelState

Represents remote MCP transport state.

```text
DISABLED
STARTING
CONNECTED
DEGRADED
RECONNECTING
ERROR
```

Tunnel state has no ownership over local Sessions. Local compute continues if tunnel state becomes unavailable.

## 13. Document revision model

An open notebook uses monotonically increasing `revision` numbers in the CoKernel document service.

Mutating operations carry an expected revision where conflict matters.

Example:

```text
revision 12
  -> human edits cell
revision 13
  -> execution output applied
revision 14
```

If external filesystem modification changes the file outside CoKernel, the document enters a conflict/reload-needed condition instead of silently overwriting the external version.

## 14. Ownership summary

| Object | Durable? | Owner/source of truth |
|---|---:|---|
| MachineRuntime desired state | yes | Windows Host state |
| Project files | yes | Project filesystem |
| dependency declaration | yes | `pyproject.toml` |
| locked resolution | yes | `uv.lock` |
| virtual environment | reconstructible | uv/Project Environment Manager |
| NotebookDocument | yes | `.ipynb` file |
| ExecutionSession namespace | no | worker process memory |
| ExecutionOperation result | partly | operation history + notebook outputs |
| FailureRecord | bounded durable diagnostic state | CoKernel Runtime |
| MCP connection | no | MCP service |
| Tunnel connection | no | tunnel runtime |

## 15. Critical invariants

1. Project determines the Python execution environment.
2. Notebook document state and Session memory state are different.
3. One notebook has one deterministic primary live Session by default.
4. Human and AI target that same primary Session.
5. Executions within one Session are serialized.
6. Different Sessions may run concurrently.
7. Environment mutation never silently destroys Session memory.
8. Session failure is contained and recorded before recovery.
9. Tunnel failure does not own or stop local compute.
10. Durable Project/Notebook state is never inferred solely from live process state.
