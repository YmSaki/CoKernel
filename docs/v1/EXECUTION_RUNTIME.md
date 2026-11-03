# CoKernel v1 Execution Runtime

Status: **implementation baseline**

## 1. Purpose

The Execution Runtime provides persistent, supervised, interactive Python execution for Project notebooks while allowing multiple notebooks/Projects to run concurrently.

The user-facing abstraction is **Execution Session**. The implementation uses one supervised Python/IPython worker process per live Session.

## 2. Core execution rules

1. A Session belongs to exactly one Project and one NotebookDocument.
2. A Session starts with one Project environment generation.
3. A Session maintains one persistent Python namespace.
4. At most one user-code operation executes in a Session at a time.
5. Requests from Human and AI enter the same Session queue.
6. Different Sessions may execute concurrently.
7. Worker crash is contained to that Session.
8. Session restart creates a new namespace and accepts in-memory state loss.
9. Notebook document writes are performed by the Runtime document service, not by the worker.

## 3. Session state machine

```text
STARTING
  -> IDLE
  -> ERROR (startup failure)
  -> CRASHED (unexpected process death)

IDLE
  -> EXECUTING
  -> STALE_ENVIRONMENT
  -> STOPPING
  -> CRASHED

EXECUTING
  -> IDLE (success/failure handled in-process)
  -> INTERRUPTING
  -> CRASHED

INTERRUPTING
  -> IDLE
  -> CRASHED

STALE_ENVIRONMENT
  -> EXECUTING (allowed, but still old process environment)
  -> STOPPING
  -> restart -> STARTING with current environment generation

STOPPING
  -> STOPPED
  -> CRASHED/ERROR if termination fails unexpectedly
```

`STALE_ENVIRONMENT` is a warning/lifecycle state, not automatic Session destruction.

## 4. Worker startup

Supervisor startup sequence:

1. Resolve Project and current ProjectEnvironment.
2. Ensure environment is `READY`.
3. Allocate `session_id` and private Unix socket path.
4. Record environment generation/fingerprint.
5. Spawn worker with Project Python interpreter using the approved worker-tooling injection mechanism.
6. Pass only non-secret runtime/session bootstrap values.
7. Worker connects to the private socket and initializes its IPython shell/namespace.
8. Worker reports bootstrap `ready` metadata including PID, Python/IPython versions, and heartbeat interval.
9. Supervisor authenticates the connected Unix peer and requires the OS-reported peer PID to match `ready.pid`.
10. Supervisor performs explicit protocol handshake/version/capability/output-limit negotiation.
11. Supervisor marks Session `IDLE` only after the handshake succeeds.

The bootstrap `ready` frame is not sufficient to make a Session usable. Startup must fail clearly if Project environment, peer identity, or worker compatibility is invalid.

## 5. Worker protocol

Transport: private Unix domain socket owned/permissioned for runtime/user identities.

Wire format for v1: length-prefixed UTF-8 JSON frames.

Worker-wire JSON must be representable by the Runtime's default `serde_json::Value` model. Integer tokens are therefore bounded to `i64::MIN..=u64::MAX`; wider Python integers are rejected before transmission rather than being allowed to become a Runtime decode failure and Session crash. Non-finite floating-point tokens are not valid worker-wire JSON.

Envelope:

```json
{
  "protocol": 1,
  "type": "request|response|event",
  "id": "uuid",
  "session_id": "uuid",
  "method": "execute",
  "payload": {}
}
```

Maximum frame size is configurable and bounded. Large rich output payloads remain bounded by execution/output policies.

### Worker requests

```text
handshake
execute
interrupt
inspect_variables
get_variable
shutdown
ping
```

### Worker events

```text
ready
execution_started
stdout
stderr
execute_result
display_data
error
execution_finished
heartbeat
worker_warning
```

The worker protocol is internal and versioned independently from MCP.

## 6. Execute request

Example payload:

```json
{
  "operation_id": "...",
  "cell_id": "...",
  "source": "x = 1\nx + 1",
  "silent": false
}
```

For `execute`, the request envelope `id` is the operation correlation ID and must equal payload `operation_id`. The worker rejects a mismatch before executing user code or emitting operation output. A successful final execute response also carries `result.operation_id`; it must equal the response envelope `id`, which in turn must equal the Runtime's active operation ID. The Runtime treats any divergence as a fatal worker protocol violation before the frame can refresh liveness or be published as a Session event.

The source comes from the authoritative NotebookDocument revision selected by the caller/runtime. The worker does not read notebook files to determine code.

## 7. Interactive execution semantics

Use IPython as the execution semantics layer.

Required v1 behavior:

- persistent `user_ns` across cell requests;
- final expression result behavior;
- stdout and stderr capture;
- Python exceptions and IPython traceback data;
- display publisher interception for MIME bundles;
- `display(...)` and rich repr paths typical of IPython;
- top-level `await` when supported by IPython;
- IPython syntax/magics needed by ordinary Python notebooks, subject to security/runtime constraints;
- shell escapes operate within the managed Linux environment as the worker identity.

The Runtime does not attempt to make notebook code side-effect free. Execution is user code with the privileges of the worker Linux identity.

## 8. Output model

Ordered output events contain monotonically increasing sequence numbers per operation.

Supported notebook output classes:

### stream

```json
{
  "kind": "stream",
  "name": "stdout|stderr",
  "text": "..."
}
```

### execute_result

```json
{
  "kind": "execute_result",
  "execution_count": 4,
  "data": {
    "text/plain": "2"
  },
  "metadata": {}
}
```

### display_data

```json
{
  "kind": "display_data",
  "data": {
    "text/plain": "<Figure ...>",
    "image/png": "<base64>"
  },
  "metadata": {}
}
```

### error

```json
{
  "kind": "error",
  "ename": "ValueError",
  "evalue": "...",
  "traceback": ["..."]
}
```

## 9. Output limits

To protect Runtime/Desktop/MCP from unbounded output:

- per-event byte limit;
- per-operation aggregate output limit;
- string length limits for safe inspection;
- configurable image/blob limit;
- truncation is explicit and recorded in result metadata;
- raw worker stdout/stderr diagnostic tails are separately bounded.

Execute terminal accounting is part of the worker protocol contract. Both `execution_finished` and the successful final execute response carry the same `output_truncated`, `output_omitted_bytes`, and `output_truncation_reasons` values. The Runtime validates them fail-closed before accepting the response.

The accounting invariants are:

- when `output_truncated=false`, `output_omitted_bytes` must be `0` and `output_truncation_reasons` must be empty;
- when `output_truncated=true`, at least one non-empty truncation reason is required;
- truncation reasons must not contain duplicates;
- `output_omitted_bytes` may be `0` when output was rejected for a non-byte-countable transport/normalization reason such as `invalid_json`;
- `execution_finished` and the final response must agree exactly on all three fields.

Exact default numeric limits are configuration constants established during implementation benchmarking and tested as contract values.

## 10. Notebook output persistence

On execution start, Notebook Document Service may update the code cell execution state in memory/event stream.

On output/finalization:

1. Runtime validates notebook/cell identity and expected revision relationship.
2. Ordered worker output events are converted to valid `.ipynb` output objects.
3. `execution_count` is updated.
4. Document revision increments.
5. File is atomically written using temp-file + replace/rename semantics on the same filesystem.
6. Document update event is published to Human/MCP clients.

A worker never directly writes its source `.ipynb`.

## 11. Execution queue

Per Session queue is FIFO by acceptance order.

Each request receives `operation_id` immediately.

States:

```text
QUEUED -> RUNNING -> SUCCEEDED|FAILED|INTERRUPTED|CANCELLED
```

Human and MCP requests use the same queue.

A request may be cancelled while queued. A running request is stopped via interrupt semantics rather than queue cancellation.

## 12. Interrupt semantics

Interrupt attempts to preserve the worker process and namespace.

Initial method:

- Supervisor sends worker interrupt control request and/or platform signal suitable for the worker process group.
- Worker/IPython converts to `KeyboardInterrupt` where possible.
- If the worker returns to command loop, Session -> `IDLE`.

If an extension/native call is uninterruptible, user can escalate to terminate/restart Session.

The current Linux implementation sends SIGINT to the verified worker PID only
after receiving its validated `execution_started` acknowledgement. An earlier
interrupt request is retained in the Session actor and delivered after that
acknowledgement, not into the worker request parser. The worker installs its
execution interrupt guard before committing the start frame. SIGINT during that
commit or execution setup becomes a structured KeyboardInterrupt/terminal result;
partially installed stream/display hooks are restored. The original namespace
survives and the Supervisor reports INTERRUPTED for the requested operation.

Outside an execution the production worker ignores SIGINT, covering a late signal
which races completion/idle framing. During execution the guard enables normal
KeyboardInterrupt behavior and defers it only across short frame/accounting
commits. SIGINT is a Runtime control signal; deliberately replacing its handler
from notebook code can change interruption behavior and is not a security
containment guarantee. Stop/restart retains the separate supervised cleanup path.

CoKernel must distinguish `Interrupt execution` from `Restart Session` in UI/API.

## 13. Stop/restart semantics

### Stop

- reject/clear queued operations with explicit cancellation status;
- request graceful worker shutdown;
- after timeout, terminate process tree;
- retain final diagnostics;
- Session -> STOPPED.

The launcher starts in a private OS process group. Runtime retains group ownership
through startup and Session lifetime, and terminates remaining ordinary descendants
even if the launcher/worker has already exited. Startup failure, stop, restart,
crash and orderly actor cancellation release that group. Deliberately detached
processes are outside this lifecycle mechanism; it is not a hostile-code sandbox.

### Restart

- same termination process;
- start a new worker using current Project environment generation;
- old volatile namespace is lost;
- NotebookDocument persists;
- Session identity may remain stable as a logical primary session with a new worker generation, or a new Session ID may be created; v1 implementation shall use a new worker-generation counter and preserve Session ID for UI continuity unless an explicit new Session is requested.

## 14. Environment changes

After `uv add/remove/sync` changes Project environment generation:

- new Sessions start with the new generation;
- existing Sessions keep running unchanged;
- existing Sessions are marked `STALE_ENVIRONMENT`;
- UI/API explains that restart is required for a guaranteed consistent view of the new dependency state;
- CoKernel does not attempt hot module reload as an environment synchronization mechanism.

## 15. Worker crash detection

Supervisor monitors:

- process handle/PID exit;
- Unix socket disconnect;
- heartbeat timeout;
- child stdout/stderr;
- worker protocol fatal errors.

Unexpected termination sequence:

1. Freeze current operation/session state.
2. Read process exit code/signal.
3. Save bounded stdout/stderr tails.
4. Query Linux OOM evidence where available.
5. Snapshot WSL memory and GPU state.
6. Correlate Runtime restart events.
7. Persist FailureRecord.
8. Session -> CRASHED.
9. Publish crash event.
10. Do not silently replace worker before evidence is available and recovery policy/user action is applied.

Runtime owns a private FailureRecord store separate from worker files and event
subscriptions. Records use a versioned JSON envelope, immutable failure ID,
atomic no-replace installation, and a synced file before crash publication.
The initial bounds are 1 MiB per serialized record and the newest 64 records per
Runtime store. Readers reject corrupt, oversized, mismatched-ID/version and
symlink records; retention removes only validated oldest records.

An absent store is empty; an unreadable or invalid store is an explicit error.
Storage failure must not prevent process cleanup: the live Session retains the
record and its persistence error, emits `FailurePersistenceFailed`, then emits
the failure event. A caller can retrieve retained evidence after subscribing
late, restarting a Session or reopening the store. Development places this store
under its temporary socket directory; production config must use durable,
Runtime-owned storage, not a Project or volatile `/run` directory.

## 16. OOM and GPU evidence

Best-effort evidence sources may include:

- cgroup/process memory measurements;
- `/proc/<pid>` while alive;
- kernel log/journal OOM records accessible to runtime diagnostics;
- process exit signal;
- NVIDIA management process/VRAM query;
- GPU health query after crash.

UI uses wording such as `OOM suspected` unless evidence is strong enough for definitive classification.

## 17. Safe variable inspection

`list_variables` and `get_variable` are worker-level constrained operations, separate from arbitrary source execution.

Rules for `get_variable`:

- input must satisfy Python identifier rules and not be a keyword;
- access only direct namespace key lookup;
- no expression parsing/eval;
- no attribute/property access;
- no calling user functions;
- do not call arbitrary `repr`/`str` on unsupported/custom values;
- serialize only approved exact built-in types (`None`, `bool`, `int` within interoperable JSON safe range `[-(2^53-1), 2^53-1]`, finite `float`, `str`, bounded exact `list`/`tuple`/`dict` with approved contents/keys);
- out-of-range integers are unsupported rather than emitted as lossy or implementation-dependent JSON numbers;
- exact type checks rather than subclass-polymorphic behavior for safe values;
- bounded recursion depth/item count/string size/total response;
- unsupported values return type name/module and `unsupported=true` without user-defined representation;
- type/module metadata lookup must not invoke custom metaclass descriptors; when metadata cannot be read without crossing that boundary, return unknown metadata.

Hostile-object tests are mandatory.

## 18. Worker process isolation/permissions

Worker runs as Project workload identity with access to:

- its Project files;
- shared uv cache as required;
- GPU device through WSL;
- network according to normal managed Linux policy.

Worker must not receive:

- Windows user files through automatic drive mounts;
- Windows secrets;
- tunnel API key;
- MCP bearer key;
- runtime privileged state writable paths;
- unrelated Project private paths where filesystem permissions can reasonably isolate them.

## 19. Parallelism policy for v1

CoKernel supports concurrent Sessions, not concurrent code execution inside one Session.

Startup/restart holds a per-notebook lock, so slow startup does not serialize
unrelated notebooks. A shared shutdown gate prevents startup from escaping a
whole-Supervisor stop. Runtime reserves pending lifecycle operations before
launching tasks and coalesces same-notebook ensure requests. Its framed admission
loop also keeps worker inspection waits separate from control requests, with 16
pending replies at most. See INTERFACE_CONTRACTS section 22 for cancellation,
Project mutation exclusion, private responses and out-of-order correlation.

Initial admission behavior:

- user may start multiple Sessions;
- runtime observes resource pressure;
- no complex automatic scheduler is required for single-machine v1;
- start/execute may warn/fail when hard runtime prerequisites are unavailable;
- future resource requests/scheduling can be layered on the Session model.

## 20. Mandatory execution tests

- state persists across two cells;
- two Sessions execute concurrently and have isolated namespaces;
- Human + simulated MCP calls serialize in one Session;
- interrupt preserves Session when possible;
- restart clears namespace;
- package mutation marks Session stale without killing it;
- normal Python exception does not crash worker;
- forced worker exit creates CRASHED + FailureRecord;
- SIGKILL/crash of one worker does not kill another;
- stdout flood/output limits behave predictably;
- rich MIME output saves valid `.ipynb`;
- hostile variable objects do not execute custom representation behavior in safe inspection.

## 21. Execution results for notebook persistence

UI event broadcast is best-effort and cannot be the source of truth for notebook
output persistence. The Supervisor now offers `execute_cell_captured`, returning
an operation ID and a one-shot completion receipt independent of subscribers.
It queues through the same per-Session FIFO as ordinary execution.

The actor collects validated ordered execution frames through the final response.
Capture is bounded to 32 MiB serialized frames and 65,536 non-heartbeat events.
Exceeding either budget discards the partial capture and returns an explicit
capture error; it does not kill the otherwise healthy worker or label partial
outputs complete. Worker output truncation within its own limits remains
explicit in the captured terminal accounting.

A crash, stop or queued-operation cancellation that prevents a complete final
response closes the receipt without a successful result. FailureRecords and
Session terminal events remain independently observable. Dropping a receipt
does not cancel accepted user code. Runtime document integration must own the
receipt/persistence task independently of a transient Desktop/MCP connection.

`ensure_primary` and `restart_primary` also reject an existing notebook mapping
whose Session belongs to a different Project, before returning or stopping it.
These checks supplement authoritative Notebook/Project validation at the domain
entrypoint; clients do not acquire arbitrary worker handles.

## 22. Whole-Runtime shutdown

The framed bridge closes admission before requesting SessionSupervisor.stop_all.
All known nonterminal Sessions receive stop concurrently and are awaited using
the existing bounded Session termination path. The document coordinator then
waits up to five seconds for its accepted capture/persistence tasks and flushes
all loaded documents. These flushes include retained documents whose Project was
removed from the registry after stopping its Sessions. Roots are already owned
by the document service; clients cannot supply replacement roots.

Shutdown errors preserve conflicts and report CK-RUN-SHUTDOWN rather than false
success. Volatile Session memory is intentionally lost on explicit Runtime stop.
Desktop disconnection must not invoke this path; Windows Host owns bridge lifetime.
The precise protocol/Host deadlines and failure limits are in INTERFACE_CONTRACTS
sections 15–16.

Execution preparation now receives a FIFO turn allocated per notebook at domain
admission. Document preparation runs through the Document Service queue; only
after that completes does the turn enqueue captured code to the Session actor.
Cancelling a middle turn removes it without releasing later turns ahead of the
head. Stop cancels pending preparation before stopping the worker; restart and
environment changes reject conflicting preparation. Output commit/save uses the
same owned document queue and retains its prior generation/ordinal checks.
The Runtime service bounds the entire explicit shutdown response to 12 seconds,
inside Host's 15-second response deadline, and preserves owned jobs on refusal.

Session control uses constant-space intent separate from user-code FIFO. One
pending interrupt is coalesced; stop and environment-stale are monotonic for the
worker generation. Admission never waits for the actor to consume control. Stop
has priority over stale/interrupt; graceful and forced process cleanup still run
under Session supervision. See INTERFACE_CONTRACTS section 25. Environment change
notification cannot be lost merely because a worker is executing or its control
consumer is temporarily stalled. New worker generations start with clear intent.

Every accepted worker execution finishes with a numeric execution_count in both
the terminal event and response. IPython may return an ExecutionResult without
a count for an empty cell or when its runner catches an early KeyboardInterrupt.
The worker then reserves the count captured before execution and advances the
shell counter to at least that count plus one, matching its outer-interrupt path.
It preserves the error/status and live namespace. Runtime validation remains
strict; legitimate early interruption must not become a worker-protocol crash.

Worker launch explicitly sets the current directory to the validated Project
root. `uv --project` chooses dependency configuration but does not itself change
the inherited process directory. Notebook relative file operations therefore
start in their own Project, never the Host/Runtime control or payload directory.
This is an initial directory, not a filesystem sandbox: user code may change its
own Session process directory without changing another Session's directory.
