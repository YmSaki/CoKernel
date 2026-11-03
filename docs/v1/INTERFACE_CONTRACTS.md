# CoKernel v1 Interface Contracts

Status: **implementation baseline**

This document defines the stable local contracts that implementation components depend on. Exact Rust type names may differ, but wire-visible semantics must remain compatible within protocol version 1.

## 1. Common envelope

All product RPC protocols use explicit versioning and request IDs.

Logical request:

```json
{
  "protocol": 1,
  "request_id": "uuid",
  "method": "projects.list",
  "params": {}
}
```

Logical success response:

```json
{
  "protocol": 1,
  "request_id": "uuid",
  "ok": true,
  "result": {}
}
```

Logical error response:

```json
{
  "protocol": 1,
  "request_id": "uuid",
  "ok": false,
  "error": {
    "code": "CK-PROJ-001",
    "component": "project",
    "summary": "Project was not found",
    "action": "Refresh the Project list or choose another Project",
    "detail": "..."
  }
}
```

Events:

```json
{
  "protocol": 1,
  "event": "session.state_changed",
  "sequence": 1234,
  "payload": {}
}
```

Timestamps on the wire use RFC 3339 UTC.

IDs are opaque strings (UUID by default) and are never parsed by UI for meaning.

## 2. Windows Desktop <-> Host

Transport: current-user Windows Named Pipe.

Per-user pipe name (canonical Windows SID, never a client-supplied user name):

```text
\\.\pipe\cokernel-v1-<current-user-SID>
```

The Host validates the connecting Windows user/ACL.

### Runtime methods

```text
runtime.get_status
runtime.start
runtime.stop
runtime.restart
runtime.get_versions
runtime.get_snapshot
```

### Project methods

```text
projects.list
projects.create
projects.register
projects.get
projects.remove_from_registry
projects.delete_files
```

### Package/environment methods

```text
packages.list
packages.add
packages.remove
packages.sync
environment.get_status
```

Package mutation methods return an asynchronous `operation_id` when resolution/install is not immediate.

### Notebook methods

```text
notebooks.list
notebooks.create
notebooks.import_file
notebooks.begin_import
notebooks.import_chunk
notebooks.finish_import
notebooks.cancel_import
notebooks.get
notebooks.apply_edits
notebooks.save
notebooks.close
```

### Session methods

```text
sessions.list
sessions.get
sessions.ensure_primary
sessions.execute_cell
sessions.interrupt
sessions.restart
sessions.stop
sessions.list_variables
sessions.get_variable
```

### Resource/diagnostic methods

```text
resources.get_snapshot
logs.query
diagnostics.export
```

### Remote access methods

```text
remote_access.get_status
remote_access.enable
remote_access.disable
remote_access.set_credentials
remote_access.clear_credentials
```

## 3. Host event subscription

Desktop connects once and receives events after an initial snapshot.

Core events:

```text
runtime.state_changed
project.changed
environment.operation_progress
environment.changed
notebook.changed
notebook.external_change_detected
session.created
session.state_changed
session.execution_queued
session.execution_started
session.output
session.execution_finished
session.failure
resources.snapshot
remote_access.state_changed
update.state_changed
notification
```

Events include domain IDs rather than UI object references.

## 4. Windows Host <-> WSL Runtime bridge

Transport: one long-lived `wsl.exe ... cokernel-runtime bridge --stdio` child process.

Framing: 32-bit big-endian length prefix + UTF-8 JSON frame.

Reason for framing instead of newline-delimited JSON: payloads may contain arbitrary newlines and future chunk metadata; framing also makes corruption detection clearer.

### Handshake

Host sends:

```json
{
  "protocol": 1,
  "request_id": "...",
  "method": "bridge.hello",
  "params": {
    "host_product_version": "1.0.0",
    "supported_protocols": [1]
  }
}
```

Runtime returns selected protocol/runtime version/capabilities.

If no compatible protocol exists, normal product operations stop with version-mismatch repair/update guidance.

### Bridge methods

The bridge mostly mirrors domain methods listed above, excluding Windows-only secret storage/UI actions.

Additional methods:

```text
bridge.ping
bridge.shutdown
runtime.health
runtime.get_linux_metrics
runtime.get_gpu_inventory
runtime.install_external_credential_handoff (ephemeral, not persisted in Project)
```

## 5. Host file streaming for Notebook import

Import uses a tokenized upload transaction.

### begin

Request:

```json
{
  "project_id": "...",
  "target_path": "notebooks/example.ipynb",
  "size_bytes": 123456,
  "sha256": "..."
}
```

Result:

```json
{
  "import_id": "...",
  "accepted_chunk_size": 262144
}
```

### chunk

```json
{
  "import_id": "...",
  "offset": 0,
  "data_base64": "..."
}
```

Chunks must be ordered or explicitly offset-validated. Runtime writes only to temporary import storage.

### finish

Runtime verifies size/hash, parses notebook, validates target containment, and atomically installs the file.

Import failure removes temporary state.

## 6. Runtime local Unix API

MCP service communicates with Runtime through a Unix domain socket rather than invoking workers directly.

Suggested path:

```text
/var/lib/cokernel/sockets/runtime.sock
```

Permissions grant only trusted product service identities.

Wire protocol can reuse the same framed JSON RPC semantics and domain method names as the bridge.

This contract deliberately lets MCP and future Node control reuse the same domain service.

## 7. Runtime <-> Worker protocol

Transport: private per-Session Unix domain socket.

Framing: same 32-bit length-prefixed JSON.

### Handshake

Worker -> Supervisor:

```json
{
  "protocol": 1,
  "type": "event",
  "event": "worker.hello",
  "session_id": "...",
  "payload": {
    "worker_version": "1.0.0",
    "python_version": "3.x.y",
    "ipython_version": "...",
    "pid": 12345
  }
}
```

Supervisor accepts/rejects protocol compatibility.

### execute

Supervisor -> Worker:

```json
{
  "protocol": 1,
  "type": "request",
  "id": "...",
  "session_id": "...",
  "method": "execute",
  "payload": {
    "operation_id": "...",
    "cell_id": "...",
    "source": "..."
  }
}
```

### output event

```json
{
  "protocol": 1,
  "type": "event",
  "session_id": "...",
  "event": "execution.output",
  "payload": {
    "operation_id": "...",
    "sequence": 4,
    "output": {
      "kind": "stream",
      "name": "stdout",
      "text": "hello\n"
    }
  }
}
```

### final event

```json
{
  "protocol": 1,
  "type": "event",
  "session_id": "...",
  "event": "execution.finished",
  "payload": {
    "operation_id": "...",
    "status": "SUCCEEDED",
    "execution_count": 7,
    "duration_ms": 123
  }
}
```

### safe variable request

```json
{
  "method": "get_variable",
  "payload": {
    "name": "secret_from_master"
  }
}
```

The worker validates identifier-only semantics itself even if upstream callers validate too.

## 8. Notebook document mutation contract

Notebook edits use revision-based optimistic concurrency.

Example:

```json
{
  "project_id": "...",
  "notebook_id": "...",
  "expected_revision": 12,
  "edits": [
    {
      "op": "set_cell_source",
      "cell_id": "...",
      "source": "x = 10"
    }
  ]
}
```

If current revision is not 12, Runtime returns `CK-NB-CONFLICT` and current revision metadata. Client reloads/rebases rather than overwriting blindly.

Execution output application is a Runtime-internal revisioned mutation using the same document authority.

## 9. Session ensure-primary contract

`ensure_primary` takes:

```json
{
  "project_id": "...",
  "notebook_id": "..."
}
```

Behavior:

- return existing live primary Session if one exists;
- otherwise start exactly one and return it;
- concurrent callers are deduplicated by notebook lock;
- never return a Session belonging to a different Project/notebook;
- no caller needs to choose a PID or interpreter path.

## 10. Execute-cell contract

Caller supplies:

```json
{
  "project_id": "...",
  "session_id": "...",
  "notebook_id": "...",
  "cell_id": "...",
  "expected_revision": 42
}
```

Runtime:

1. validates Session belongs to Notebook/Project;
2. validates the cell and revision (or returns conflict/current revision);
3. fetches authoritative source;
4. creates `operation_id` and queues it;
5. immediately returns queue acceptance;
6. publishes output/final state through events.

This contract prevents arbitrary caller source from masquerading as an existing cell execution.

The current Linux bridge returns a `DocumentOperation` at acceptance and exposes
`operations.get {project_id, operation_id}` for completion. Its `result.state` is
`PENDING`, `PERSISTED` (with `execution_status` and saved `revision`), or
`NOT_PERSISTED` (with optional `execution_status` and structured `error`). Records
are bounded and Runtime-lifetime; see PROJECT_NOTEBOOK_MODEL section 23. Session
output events are forwarded by Host as described in section 19. Completion of
Python execution and completion of document persistence are separate events.

## 11. Error code families

```text
CK-INST-*  install/bootstrap
CK-HOST-*  Windows Host/bridge
CK-RUN-*   WSL Runtime
CK-PROJ-*  Project
CK-UV-*    environment/package
CK-NB-*    notebook document/import
CK-SES-*   Session/supervision
CK-SESS-*  implemented Session admission/inspection adapter errors
CK-EXE-*   execution operation
CK-MCP-*   MCP service/tool
CK-TUN-*   remote tunnel
CK-GPU-*   GPU/resource
CK-SEC-*   permission/auth/security
CK-UPD-*   update/repair
```

Errors are stable enough for UI handling/tests; raw subprocess exits belong in `detail/evidence`.

## 12. Cancellation

Long-running operations (uv sync, import, diagnostics export, update) return `operation_id` and support cancellation where underlying operation can be safely cancelled.

Cancellation is not silently mapped to Session interrupt unless the operation is an execution operation.

## 13. Protocol compatibility policy

- protocol major (`protocol: 1`) changes only for incompatible wire semantics;
- new optional fields/methods may be added within protocol 1;
- receivers ignore unknown optional fields unless strict validation is security-relevant;
- capability handshake allows older/newer Host/Runtime to detect unsupported operations explicitly;
- release packaging should normally keep Host/Runtime versions aligned.

## 14. Implemented notebook bridge methods

Linux `bridge.hello` advertises `notebooks`, `sessions`, `notebook-import` and
`operation-status`. Every notebook method carries a registered `project_id`;
paths are Project-relative. Notebook/session-specific parameter structs reject
unknown fields. `sessions.execute_cell` also rejects `source` and `origin`.
Revision conflicts return `CK-NB-CONFLICT`; the existing string `error.detail`
contains JSON `{"current_revision":42}` for machine-readable reconciliation.

| Method | Additional parameters | Result |
|---|---|---|
| `notebooks.list` | `offset=0`, `limit=256` (1..512) | `total`, `offset`, `paths` |
| `notebooks.create`, `notebooks.open` | `relative_path` | Document summary |
| `notebooks.get` | `notebook_id` | Full snapshot, flattened document identity/revision and `notebook` JSON |
| `notebooks.apply_edits` | `notebook_id`, `expected_revision`, `edits` | Document summary |
| `notebooks.save`, `notebooks.close`, `notebooks.reload` | `notebook_id`, `expected_revision` | Document summary |
| `notebooks.begin_import` | `target_path`, `size_bytes`, `sha256` | `import_id`, `accepted_chunk_size`, `expires_in_seconds` |
| `notebooks.import_chunk` | `import_id`, `offset`, `data_base64` | `next_offset` |
| `notebooks.finish_import` | `import_id` | Document summary |
| `notebooks.cancel_import` | `import_id` | `cancelled` |
| `notebooks.read_chunk` | `notebook_id`, `expected_revision`, `offset`, `length` (1..262144) | `revision`, `offset`, `total_bytes`, `sha256`, `data_base64` |

A document summary contains `document` (NotebookDocumentRef), `state`,
`content_hash` and `saved_content_hash`. Content hashes and read chunks refer to
the current canonical JSON bytes, including their final newline. Read chunks
require an unchanged revision; offset equal to total length returns empty data.
A full response above the 8 MiB frame limit returns `CK-NB-LIMIT` with a chunked
read action. It does not terminate the Runtime. Chunk/import limits are defined
in PROJECT_NOTEBOOK_MODEL sections 21–23.

`sessions.ensure_primary` and `sessions.restart` take Project/Notebook IDs;
`sessions.get`, `sessions.stop`, `sessions.interrupt`, `sessions.list_variables`
take Project/Session IDs; `sessions.get_variable` also takes identifier `name`.
`sessions.list` takes Project ID. All reject cross-Project Session access.
Successful package add/remove/sync marks live Sessions environment-stale without
discarding memory. Removing a Project from the registry while it has a live
Session returns `CK-PROJ-IN-USE` and requires explicit Session stop first.

The framed stdio adapter admits requests serially. Execution and package
add/remove/sync return operation IDs; uv runs outside this admission loop.
Completion is serviced alongside input reads. Project create/register likewise
return operation IDs before their init/sync subprocesses run. The bridge's internal
Project ownership checks do not replace transport auth.

## 15. Host bridge ownership and recovery baseline

`cokernel-host serve <wsl-config.json> <absolute-state-directory> [--start|--stop]`
starts the user-level Host independently of Desktop. The service binds its
current-user Named Pipe before starting the supervisor and also emits status JSON
to its console. Runtime events/snapshots use section 19. Config is a
bounded local file containing `distribution`, `linux_user`, and absolute Linux
`runtime_path`; it is not accepted through client RPC. Launch uses explicit
`wsl.exe --distribution ... --user ... --exec ... bridge --stdio` arguments and
CREATE_NO_WINDOW, never a concatenated shell command. Developer acceptance may
use an isolated Runtime wrapper; the installer supplies the production payload.

The Host clears inherited launch environment and restores only OS launch/path
essentials. WSLENV and arbitrary credential variables are not forwarded.
Subprocess stderr is drained without retaining or echoing its content. Structured
Host errors describe the operation and recovery action without copying raw
subprocess diagnostics. Runtime FailureRecords remain the domain evidence source.

Before exposing a bridge, Host performs a 20-second bounded protocol-1 handshake.
Up to 32 valid Runtime events may precede the hello response. Host consumes them
without retaining payloads, validates protocol/name/strict sequence order, and
carries the sequence watermark into ordinary event reception. No subscriber exists
during handshake; the initial authoritative snapshot supplies current state.
Excess events, malformed events, sequence rollback (including across hello), or
the existing handshake deadline fail the connection. Startup observations must
not be misparsed as hello responses when independent services start concurrently.
Version/malformed-envelope mismatch stops recovery until explicit start/repair.
Frames are bounded at 8 MiB before payload allocation. Host issues new UUID request
IDs and correlates replies independently of caller completion order. At most 32
requests wait in the queue and 32 await replies. The private read queue holds at
most eight frames. Events have increasing sequence numbers and a 256-event
best-effort subscription buffer; lag is explicit through the receiver API.
Runtime event forwarding and initial snapshots use section 19; concurrent
Session request completion uses section 22.

Dropping a caller future does not cancel a submitted request or drop the bridge.
Default request timeout is 300 seconds; a write is bounded to ten seconds.
Timeout, framing corruption, unknown response ID, invalid response shape or
transport loss invalidates the connection and completes pending calls with an
explicit uncertain-outcome error. No request is replayed automatically. A user
must inspect current document/operation state before deciding whether to retry.

Recovery delays are 1, 2, 4 and 8 seconds, followed by ERROR on the fifth
consecutive failure. Sixty healthy seconds reset the failure counter. Generation
increments for each successful connection within the Host lifetime; last_error
survives recovery so the replacement is visible. A replacement Runtime has fresh
volatile Session memory and Runtime-lifetime notebook/operation IDs; the Host
does not silently reconstruct live Python variables or replay cells.

Host owns an exclusive state-directory lock and persists schema-1 desired
RUNNING/STOPPED before applying a user request. Missing state defaults to STOPPED;
corrupt/oversized/unknown-schema state fails explicitly. Persistence uses a unique
temporary file, file sync and atomic replacement (directory sync also on Unix).
This is tested process/reopen persistence, not a hardware power-loss guarantee.
Failed persistence does not apply the requested state. Host exit preserves desired
state for the next sign-in/start; explicit Stop persists STOPPED. Local CLI override
is saved before launching the supervisor, so --stop cannot transiently start a
previous RUNNING configuration.

## 16. Runtime bridge termination

`bridge.shutdown` requires protocol 1 and empty parameters. Runtime stops public
stdio admission, signals every Session to stop, waits for supervised cleanup,
settles accepted output-persistence tasks, and flushes Runtime-owned documents.
The successful response is `{"stopped":true}` and the process exits. Session
cleanup timeout or document conflict/save failure returns CK-RUN-SHUTDOWN and
keeps the bridge/document memory available for explicit reconciliation and retry;
Sessions may already have stopped. Files changed externally are not overwritten.
An explicit shutdown while a package operation is active returns CK-UV-BUSY
before stopping Sessions; clients may wait or cancel that operation and retry.
EOF and malformed bridge input cancel/reap active package subprocesses, reconcile
their environment state and attempt the same Session/document cleanup before
exiting. This includes Project create/register init/sync subprocesses.

Host waits for the correlated shutdown response before closing stdin. A refusal
returns CK-HOST-SHUTDOWN-REFUSED and retains the connection even when desired
state is STOPPED; document inspection/reconciliation RPC remains available.
Host exit is also refused while the Runtime can retain unsaved document memory.
After successful acknowledgement Host closes stdin and waits up to 15 seconds
before terminating its owned launch process. A forced/nonzero exit is not reported as clean stop.
This does not assert that force-killing wsl.exe is a Linux descendant containment
mechanism; normal Runtime shutdown explicitly owns worker/process-group cleanup.
Abrupt Host/Runtime crash and managed-distro repair remain broader acceptance
cases. Host shutdown does not call wsl --shutdown or terminate unrelated distros.
Unexpected EOF still has only best-effort final flushing; it cannot preserve
volatile unsaved edits when storage is unavailable. Durable edit recovery across
abrupt Runtime/Host loss is not established by this lifecycle component proof.

## 17. Implemented Windows local admission and client lifetime

The Host creates a non-inheritable, local-only pipe with a protected DACL allowing
only its current Windows user SID. It uses first-instance ownership and keeps a
listening instance alive while accepting another connection. A second Host cannot
claim that name. After reading the initial frame, Host synchronously impersonates
the peer, checks its token SID and reverts before any await. Failure to revert is
fatal because continued execution with the peer identity is unsafe. The supplied
client also checks the server process token SID before sending requests. These
checks enforce the single-user product boundary, not isolation from an
administrator or malicious processes already running as that user.

The first RPC is `host.hello` with exactly `{"supported_protocols":[1]}`. Reply
includes `selected_protocol`, `host_product_version`, and capabilities
`host-status`, `runtime-control`, `domain-rpc`, `host-state-events`, `runtime-events`,
`runtime-snapshot`, and `windows-notebook-import`. Empty parameters are required for runtime control.
`runtime.get_versions` returns protocol, Host version and nullable Runtime version.
Internal bridge/lifecycle/credential methods and arbitrary launch commands cannot
be invoked through this transport. Domain requests use the same Host-owned bridge.

After hello, `host.snapshot` sequence 0 contains current `host` status. Subsequent
`runtime.state_changed` events contain the latest Host status with increasing
connection-local sequence numbers. This status watch may coalesce transitions;
it is not an audit history. A reconnect starts at sequence 0 with a fresh snapshot.
When Runtime is healthy, the initial payload additionally contains its compact
`runtime` snapshot. If that read fails, is unavailable or crosses a Host generation
change, `runtime` is null and `runtime_snapshot_required` is true. Host control
remains usable; no empty successful Runtime state is invented.

Admission bounds: 16 clients, 32 requests globally, eight pending per client,
8192 distinct IDs per connection, 128-byte nonempty request IDs/method names,
8 MiB frames, five-second hello/write limits and ten-second frame body reads.
Request IDs cannot be reused within one connection. Each reader owns its entire
frame read, so unrelated events cannot cancel a partial frame. The output queue
holds eight messages and allows five seconds to drain before disconnecting a
stalled client. Temporary bursts yield to the writer; a full queue alone does not
immediately disconnect a healthy draining client. Runtime continues independently.
Excess requests return CK-HOST-BUSY. Malformed framing or duplicate IDs close only
that connection. Submitted domain tasks belong to Host and survive client closure.
The sequential Rust client marks its connection unusable after a cancelled/failed
read or RPC, requiring reconnect rather than consuming a partial or stale reply.
Its 256-event buffer reports `events.resync_required` on overflow.

Windows API basis: [pipe access checks](https://learn.microsoft.com/en-us/windows/win32/ipc/named-pipe-security-and-access-rights)
and [RevertToSelf failure handling](https://learn.microsoft.com/en-us/windows/win32/api/securitybaseapi/nf-securitybaseapi-reverttoself).

## 18. Host-owned Windows notebook import

`notebooks.import_file` is a Windows Host-only method with exactly `project_id`,
`target_path` (Project-relative), and `windows_path` (selected absolute local disk
path). It returns the Runtime's document summary. It is not a Runtime or MCP tool.
Host never forwards `windows_path`, translates it to /mnt paths, or mounts a drive.

Host accepts a nonempty regular `.ipynb` of at most 64 MiB. Device, UNC/network,
relative, alternate-stream and final reparse-point paths are rejected. The opened
object is checked in addition to lexical path validation. Source handles allow
read sharing only; concurrent write/delete/rename access is excluded for the
hashing/transfer lifetime. Disk reads and hashing run on blocking workers, at most
four imports concurrently. Only bounded byte chunks (at most 256 KiB, base64),
SHA-256, size and Project-relative destination cross the Runtime bridge.

Host uses begin/chunk/finish with validated offsets and Runtime-owned transaction
IDs. Runtime remains responsible for notebook format, safe destination resolution,
collision refusal and atomic install. Failed transfers attempt cancellation of
their own transaction; the Runtime's existing TTL handles transport loss. Nothing
is replayed after uncertain outcomes. A disconnected Desktop does not cancel an
already accepted import; reconnect and inspect the target before retrying.
Malformed parameters or source-read failures return CK-HOST-IMPORT without copying
the Windows path into diagnostics. Runtime errors retain their domain codes.

Windows API basis: [CreateFile sharing and reparse behavior](https://learn.microsoft.com/en-us/windows/win32/api/fileapi/nf-fileapi-createfilew)
and [opened-object path resolution](https://learn.microsoft.com/en-us/windows/win32/api/fileapi/nf-fileapi-getfinalpathnamebyhandlew).

## 19. Runtime observations and reconnect snapshots

Runtime's shared EventHub assigns one monotonic sequence across all Sessions and
Document operations. It retains at most 256 observations, each at most 32 KiB.
Oversized observations become `events.resync_required {reason:"event_size_limit"}`;
rich output is never silently truncated into a supposedly complete event. Durable
failure records and captured notebook execution receipts remain independent of
subscriptions, lag, transport errors and event size limits.

Implemented events include `project.changed`, `environment.changed`,
`notebook.changed`, `notebook.refresh_required`, `session.state_changed`,
`session.execution_queued`, `session.execution_started`, `session.output`,
`session.execution_finished`, `session.failure`, `session.failure_persistence_failed`,
and `notebook.execution_completed`. Worker heartbeat and private inspection
responses are excluded. Session events include Project/Notebook/Session identity;
worker output carries `kind` and structured worker `data`. Failure notifications
carry identity/classification, not raw diagnostic tails.

`session.execution_finished` reports execution status. The subsequent document
completion event contains `operation_id`, identity and `result.state` of PERSISTED
or NOT_PERSISTED, including revision or save/conflict error. Consumers must not
equate Python completion with saved outputs. `operations.get` and document reads
are authoritative recovery paths when a completion event is missed.

A dedicated bounded writer serializes complete event and response frames. A
cancelled response/event selection cannot interrupt a partial frame write. If
its subscription lags, the writer atomically resubscribes at the current watermark
and emits a resync marker before newer events. It does not emit lower sequence
numbers from the discarded retained suffix after that marker.

Host owns one event forwarder per bridge generation. Replacing/stopping the bridge
retires its forwarder. Forwarded pipe events preserve their Runtime event name;
payload is `{generation, runtime_sequence, data}`. The outer envelope sequence is
connection-local; `runtime_sequence` belongs to the indicated Host generation.
Lag markers generated inside Host may have no Runtime sequence. Slow observers
never block the bridge reader or worker. Buffer lag is explicit; a pipe which
cannot drain even after bounded backpressure is closed and must reconnect.

`runtime.get_snapshot {}` returns `events_before`, `events_after`, compact
`projects`, `sessions`, open `documents`, `project_total` and `session_total`.
Project/Session arrays are bounded to 256/512 entries; totals expose incomplete
lists. Open document summaries use the existing 128-document bound and do not
copy cells/outputs. This is a live observation across domain objects, not an
atomic whole-Runtime transaction. A client subscribes before requesting the
snapshot, buffers observations, installs it and applies matching-generation
events newer than `events_before`; it reconciles by document revision and performs
another domain read after any gap. On generation change it discards stale live
Session identities and obtains a new snapshot. Truncated arrays require scoped
Project/Session reads; they must not be shown as complete inventories.

Host performs this read for a healthy initial/reconnect pipe snapshot, and exposes
the same explicit method for resynchronization. Snapshot now also includes
`package_operations`, the bounded retained package history described below.

## 20. Asynchronous package operations

Linux Runtime with notebook services advertises `async-packages`.
Host exposes the connected Runtime capabilities and generation in
`runtime.get_versions` and Host status/snapshot, clearing them when disconnected.
`packages.add`, `packages.remove` and `packages.sync` always return a PACKAGE
operation record after admission, before waiting for uv. Parameters reject unknown
fields. Add/remove accept 1–128 specifications, each at most 4096 bytes, without
control characters or leading command options. Arguments follow an explicit `--`
separator. Project paths are resolved under the managed root before launch and
revalidated during reconciliation.

The record contains `operation_id`, `project_id`, `kind:"PACKAGE"`, `method`,
`cancellation_requested` and `result`. Result states are RUNNING, SUCCEEDED
(with the updated Project record), or FAILED (with structured `error` and optional
`reconciliation_error`). `operations.get {project_id,operation_id}` returns the
same record. Project ownership is checked before returning details.

There is at most one environment mutation per Project and four across Runtime.
Conflicting mutation/Project removal returns CK-UV-BUSY; a busy response creates
no operation and starts no uv process. The latest 256 records are retained for
the Runtime lifetime, evicting only terminal records. Evicted IDs are not found.
No operation is silently replayed after reconnect or Runtime restart.

`environment.operation_progress` publishes RUNNING admission and terminal state;
it reports lifecycle progress, not invented resolver/install percentages.
`environment.changed` is a refresh hint after reconciliation. Completion does
not require another request or polling. Input framing retains a partially read
header/body across completion handling. Once a completion is taken, registry
reconciliation and Session stale notification finish before another request is
admitted. Existing execution/inspection and control remain available while uv runs.

Before launching uv, Runtime atomically persists the Project's
`environment_reconcile_required:true`. The registry writes a unique temporary
file, syncs it, replaces the destination and syncs the parent directory on Unix.
This marker survives process restart; hardware power-loss durability is not
claimed by the local test. Successful uv reconciliation updates the dependency
fingerprint/generation and clears the marker. Failed/cancelled/timed-out work
retains it and advances generation conservatively, even if dependency files did
not change, because an environment may have been partially modified. A failure
to reconcile also marks live Sessions uncertain without killing their workers.

`environment.get_status` includes `reconcile_required` and reports `ready:false`
while that flag is set. Existing primary Session lookup and execution remain
available. New Session startup/restart is refused with CK-UV-BUSY during mutation
or CK-UV-NEEDS-SYNC after uncertain completion. A successful package sync/change
clears the condition; explicit Session restart is still required to adopt it.

`operations.cancel {project_id,operation_id}` is for package operations. It returns
`accepted:true` while an owned operation is running, signals only that uv process
group and emits `environment.operation_cancel_requested`. It does not interrupt
Python, roll back package files or guarantee cancellation wins a race with an
already finished command. Terminal operations return `accepted:false`; an
execution operation ID is not converted into a Session interrupt. Actual uv
cancellation ends with FAILED / CK-UV-CANCELLED and requires environment repair.

Production uv has null stdin, a 30-minute deadline and a 64 KiB bound per output
stream. Timeout/output overflow are CK-UV-TIMEOUT / CK-UV-OUTPUT-LIMIT. The Linux
runner owns a private process group, drains stdout/stderr independently, terminates
ordinary build descendants and reaps the child before committing completion.
Raw output is not streamed to all clients; error detail is bounded to 8192 bytes.
This is process ownership, not containment of malicious processes that detach.

Long Session startup/inspection replies use section 22. The standalone Project
CLI retains synchronous command semantics.

## 21. Asynchronous Project setup and recovery

Linux Runtime advertises `async-projects`. `projects.create {name}` and
`projects.register {path}` return an operation with `kind:"PROJECT"`, Project ID
and operation ID before waiting for uv. They share section 20's capacity, ownership,
history, get/cancel, EOF cleanup and lifecycle event contracts. Clients wait for
SUCCEEDED and read its Project record before starting a new Session. Existing
tools must inspect capability flags rather than assume the old immediate record.
The snapshot field `package_operations` includes these environment/setup records.

Creation reserves a new directory and Project identity, initially generation 0,
with `environment_reconcile_required:true` and `initialization_required:true`.
It then runs uv init and uv sync sequentially in the background. Success reconciles
generation and clears both flags. A duplicate name/path does not create another
operation or overwrite an existing directory. The four-operation global bound is
checked before reserving new paths. Registration validates a managed existing
path and pyproject, reserves identity/uncertainty, and synchronizes asynchronously.
Re-registering an existing identity explicitly synchronizes it, using that same
ID and normal mutation exclusion; it does not create a duplicate registry entry.

Cancellation/failure retains the Project identity and files. It never recursively
deletes a partially initialized Project. `packages.sync` resumes initialization
when `initialization_required` is set: it runs uv init only if pyproject is still
absent, then syncs. Existing pyproject declarations and user files are preserved.
Add/remove is refused with CK-UV-NEEDS-SYNC until initialization completes.
An uncertain/incomplete Project cannot start a new worker. Both flags persist
across Runtime restart and remain visible through Project reads.

An admission registry-write failure returns an error and launches no uv command.
The new directory and uncertain identity remain visible in the live Runtime for
repair; `project.refresh_required` requests a list refresh. Restore storage access
and synchronize that Project. If the registry could not be persisted at all, its
in-memory ID is not guaranteed across Runtime restart; retained files are not
claimed to be a successfully created Project. Release repair UI must expose this
storage failure clearly. No power-loss or destructive rollback guarantee is implied.

## 22. Concurrent Session request completion

Linux Runtime advertises `concurrent-session-rpc`. Admission validates requests
serially, but `sessions.ensure_primary`, `sessions.restart`, `sessions.get_variable`
and `sessions.list_variables` await their worker work outside the admission loop.
Their existing success shapes are unchanged; each response carries the original
request ID and may arrive out of order. Clients must correlate by ID, not arrival
order. Inspection results remain private responses and are never event payloads.

At most 16 owned-work responses are pending or queued for delivery. Section 24
extends this shared budget to document work, snapshots and execution preparation.
Further work returns CK-RUN-BUSY; status, interrupt and stop remain available.
Transport limits apply independently. A caller disconnect does not cancel accepted
work. Inspection waits until the Session actor can inspect its namespace between
executions; it does not promise a snapshot taken at request admission.

Concurrent ensure-primary requests for one notebook share one startup result.
A second explicit restart during a lifecycle operation returns CK-SESS-STARTING.
Different notebooks may start concurrently; Supervisor locks are per notebook,
with a shared shutdown gate. Existing crashed primary Sessions are returned with
their evidence rather than silently restarted. Explicit restart adopts the latest
reconciled Project environment and discards the old namespace.

Startup/restart reserves the notebook lifecycle. Package mutation, re-registration
and Project removal are refused with CK-SESS-STARTING while that Project has a
startup pending. Execution/inspection targeting a restarting notebook is likewise
refused. Ownership and strict parameter validation occur before starting work.
The `session.starting` event carries only Project and Notebook IDs; snapshot
`session_requests` contains `pending_requests` and `starting_notebooks` with those
IDs, without variable names, values or private caller IDs.

Stop on an existing Session cancels and joins any pending notebook restart/start,
then stops its current worker. Cancelled requests receive a structured Session
error, including CK-SESS-REQUEST-CANCELLED for an aborted task. Explicit bridge
shutdown cancels/joins pending Session requests before stopping all Sessions and
flushing documents. Active environment operations still cause the prior section
16 refusal before cancellation. EOF cancels pending requests, reaps their owned
launchers/groups, then performs environment and active-Session cleanup. The bridge
retains partial input frames while servicing both Session and package completions.

## 23. Shared Runtime endpoint ownership

Production stdio now submits through the same transport-independent
`RuntimeHandle` API intended for the Runtime Unix API/MCP adapter. `submit`
enqueues a request and returns a response ticket; `call` awaits that ticket.
Neither requires a mutable borrow of the Runtime service. `RuntimeOwner` owns
domain admission, operation completion and response delivery. The bridge handles
framing and response tickets without driving package or Session task collections.

The endpoint has a 32-request total bound including queued, in-flight and
completed-but-unread replies. It validates the existing 8 MiB frame limit for
direct callers too. Capacity excess returns CK-RUN-BUSY before admission. Worker
reply limits from section 22 apply independently. The stdio adapter also limits
its reply tasks to 32, so consuming tickets cannot create unbounded transport
backlog. An internal UUID prevents identical caller request IDs on separate
connections from colliding; only the original ID is returned to that caller.
The authenticated adapter supplies ExecutionOrigin separately from JSON.

Dropping a handle or ticket does not stop Runtime or cancel admitted work. The
daemon retains RuntimeOwner independent of clients. Owner finish/stdio EOF closes
admission, rejects queued-but-unadmitted requests with CK-RUN-STOPPED and joins
accepted-work cleanup. A successful bridge.shutdown closes the endpoint; a
rejected shutdown leaves it available for explicit document/environment repair.
The stdio transport sends resolved pending replies before the final shutdown ack.

Linux bridge stdin uses a duplicated close-on-exec descriptor with nonblocking
readiness, not a blocking stdin thread. Its input must be readiness-capable (the
Host/stdio pipe); unsupported descriptors fail at bridge startup. Pending reads
can be dropped after shutdown without waiting for the parent to close its writer.
The same read future is retained across partial frames until the request completes
or the bridge terminates. This changes no worker stdin or socket ownership.

This is the common client/lifetime boundary. Section 24 moves Document Service
waits into its owner. Section 25 removes control queue capacity waits; section 26
moves Project registry work into its owner. Explicit Runtime shutdown still joins
owned work under its response budget.
The outstanding inventory and required tests are tracked in DELIVERY_PLAN.

## 24. Owned document work and execution preparation

Linux Runtime advertises `owned-document-io`. Notebook load/create/import/read/
edit/save/close/reload and document snapshots are submitted in admission order to
the Document Service's FIFO queue. A queue owner joins each filesystem job on the
blocking pool before taking the next. Public admission uses non-waiting capacity
reservation; at most 32 jobs are queued plus one running. Accepted execution
persistence and shutdown flushes may wait for a queue slot under their existing
execution/lifecycle bounds. Client cancellation never cancels accepted file writes.

Document jobs return through the same 16-response request-work collection as
Session startup and inspection. Snapshot `request_work` is the common summary;
the earlier `session_requests` field is retained as an alias. Counts now include
document/snapshot/preparation replies. No caller IDs or variable values appear.
Document parameters and Project ownership retain their previous validation,
atomic persistence and revision rules. Project removal while document work is
pending returns CK-NB-BUSY. Unknown document methods retain CK-RUN-NO-METHOD.

Ensure-primary validation and execution preparation also use the document queue.
An in-memory NotebookId-to-ProjectId index rejects ownership mismatches before
reserving startup. It contains no document contents or paths; the document job
still performs authoritative filesystem validation before launching a worker.
Execution preparation reserves a per-notebook FIFO turn during admission, before
spawning its reply future. That turn covers preparation and enqueueing user code,
not the execution duration. Removing a cancelled middle turn cannot let later
code overtake a still-pending first turn. Different notebooks have separate turns;
worker execution retains the existing per-Session FIFO.

Restart/ensure on a notebook with pending execution preparation returns
CK-SESS-PREPARING. Project environment mutation/re-registration/removal is likewise
refused while that Project has preparation pending. Stop cancels/joins pending
preparations and lifecycle starts before stopping the Session, so releasing slow
document work afterward cannot start previously cancelled code. Source and
generation checks still reject stale execution output commits after edits.

Runtime shutdown cancels interruptible startup/inspection/preparation work but
drains accepted document requests. An explicit shutdown whose document requests
do not drain within five seconds returns CK-RUN-SHUTDOWN and keeps admission and
the document owner available for recovery. Document flush likewise has a
five-second wait bound; its accepted job continues under the owner if the caller
times out. Retry after storage recovers. EOF/owner termination joins owned work
without claiming completion before writes settle; this is not a guarantee against
an operating-system or hardware I/O hang. Explicit Runtime shutdown has a total
twelve-second response budget inside the Host's fifteen-second deadline. Timeout
does not cancel accepted document jobs or their persistence owners. No worker
gains a document-writing path.

## 25. Session control intent and responsiveness

Interrupt, stop and environment-stale notifications do not wait for execution,
inspection or control queue capacity. Each Session holds one pending interrupt
and two monotonic flags (stop and stale) for its current worker generation.
Repeated pending interrupts coalesce; after consumption a new interrupt can be
admitted. Repeated stop/stale requests keep their existing intent without adding
jobs, allocating an unbounded queue or spawning another sender task.

The actor observes stop before stale and interrupt. Stop still cancels queued
work and owns bounded worker/process-group cleanup; an `accepted` response is
control admission, not a claim that the process has already exited. Interrupted
execution retains its existing execution-start acknowledgement and SIGINT guard.
An idle interrupt remains a no-op. An exited actor rejects control requests.

Stale notification is retained while code executes and produces STALE_ENVIRONMENT
when execution settles. It never restarts or kills a Session. A replacement worker
receives fresh control state, so a previous generation's stop/stale intent cannot
affect it. Human/AI user-code FIFO and distinct-notebook parallelism are unchanged.
Runtime lifecycle cancellation still joins bounded owned tasks before stopping;
Project registry storage ownership is specified in section 26.

## 26. Owned Project registry work

Linux Runtime with Notebook services owns one Project filesystem queue and one
read-only registry projection. No cloned mutable registry managers may write
independent snapshots. The owner serializes canonical-path resolution, prepare,
write-ahead uncertainty, environment status probes, reconciliation and registry
removal on the blocking pool. Public admission is non-waiting, bounded to sixteen
queued storage jobs plus one running. At most four environment operations,
including preparation, are admitted; their owned reconciliation may wait for a
storage slot. The shared request/reply limits still apply.

Project preparation uses the common request-work completion path. It may await
storage before returning the existing operation-ID receipt, while unrelated
Session status/interrupt/stop continue. uv starts only after successful durable
write-ahead preparation. The operation stays RUNNING and the Project remains
reserved through uv completion and registry reconciliation. Only then are stale
notifications and terminal operation events published. Projection updates precede
storage replies and include retained uncertain identities after failures; they
never present a successful reconciliation before its persistence attempt finishes.

Create reserves its managed target path. Register first resolves the canonical
path through the owner, then checks current lifecycle/environment reservations
before scheduling preparation. Canonical aliases share a reservation. A Project
may not acquire another environment mutation while preparation/reconciliation or
registry removal is pending. Existing Project create still returns CK-PROJ-EXISTS.

Registry removal checks live Sessions and pending document/lifecycle work, then
reserves identity until its write completes. New document work and Session
startup cannot pass this reservation. No Project file is deleted. A failed write
restores the record in memory for retry; a failure after rename does not establish
durability of either registry version until reconciliation/retry succeeds.

Dropping a reply does not cancel accepted storage writes. Owner/transport EOF
closes environment launch before draining preparation: newly prepared identities
remain repairable but no new uv process starts during cleanup. Already running
uv operations are cancelled/reaped and reconciled through the owner. Explicit
shutdown refuses while environment preparation/removal is active, as it does for
running uv operations. A Project-owner failure returns CK-PROJ-OWNER and makes
its projection uncertain; a full queue returns CK-PROJ-BUSY without mutation.
Existing Session memory/control is independent of this owner's availability.

The synchronous Project CLI/component harness remains separate. Production
Runtime-owned storage rejects accidental synchronous dispatch with
CK-RUN-OWNED-OPERATION; transport clients use RuntimeHandle.

## 27. Current-user Host autostart settings

Windows `host.hello` advertises `host-autostart` when the Host has its trusted
executable/configuration/state launch binding. `host.autostart.get` takes `{}`;
`host.autostart.set` takes exactly `{"enabled":boolean}`. Unknown fields, paths,
commands and non-boolean values are rejected with CK-HOST-REQUEST. Clients cannot
choose a startup executable, registry location, configuration path or state path.
These are Host settings, not Runtime/MCP-forwarded methods.

Both return `{registered, matches_current_launch, effective_enabled}`. `registered`
means the bounded REG_SZ product entry exists; `matches_current_launch` compares
its exact command with the trusted binding. `effective_enabled` is false when
absent and null when registered because Windows Startup Apps/policy is independent.
Malformed/oversized values, absent trusted configuration, invalid paths/configuration
and registry failures return CK-HOST-AUTOSTART (invalid WSL configuration may retain
its CK-HOST-CONFIG family). No raw foreign registry value is included in errors.

Registry/configuration work runs on the blocking pool under the Named Pipe's common
request bound. Disconnect does not cancel an accepted setting operation. The only
write target is HKCU's fixed CoKernelHost Run value. Absolute Unicode paths are
quoted directly without a shell; control characters/quotes are rejected and the
Windows command length is bounded. Enable/disable leaves persisted desired state,
live Runtime and Sessions unchanged. A newly started Host reads existing desired
state normally; missing state defaults to STOPPED. Background launch remains alive
across all Desktop/AI client disconnects.

## 28. Cached resource observations

Host `resources.get_snapshot` and Linux Runtime `runtime.get_linux_metrics` take
exactly `{}`. Client paths, commands and sampling parameters are rejected. Host
advertises `resource-snapshot`; production Linux Runtime advertises `linux-metrics`.
Resource models reside in `cokernel-domain::resources`, with collection in the
shared Rust `cokernel-metrics` crate. No worker is started to read resources.

Runtime returns `{sequence, age_ms, stale, collecting, resources}`. A collector
starts lazily on first use and has one OS/NVIDIA collection in flight. Reads return
the cached snapshot immediately. Before the first sample, sequence is zero,
resources/age are null and stale is true. CPU requires two counter samples; its
first observation is WARMING_UP. Age uses a monotonic clock from collection start;
sampled_at is RFC 3339 UTC. More than fifteen seconds old, or a stopped/failed
collector, is stale. Sampling is approximately two seconds while observed, ten
seconds in the background. Repeated reads cannot multiply collectors or subprocesses.

Each metric is `{value, unavailable}`: a measured value has no unavailable reason;
an unavailable value is null with WARMING_UP, NOT_SUPPORTED, NOT_INSTALLED,
READ_FAILED, INVALID_DATA, TIMED_OUT or COLLECTOR_FAILED. Unavailable never means
zero. Resources include source, sampled_at, cpu_percent, memory, storage,
cgroup_memory, gpus and process_gpu_attribution.

Host returns `{host, linux, linux_unavailable, runtime_generation, runtime_state,
storage_scope}`. Host and Linux are separate snapshots; the Host never adds their
memory or GPU usage. Its Linux cached-snapshot RPC has a 750 ms budget and returns
partial Host data on Runtime unavailability, unsupported capability, timeout or
invalid reply. A changed Runtime generation/state discards the old Linux reply.
Reading resources while STOPPED does not start WSL. UI can poll this method at the
dashboard cadence independently of resource event subscriptions.

WINDOWS_HOST physical memory is total minus available RAM; LINUX_RUNTIME memory
uses MemTotal/MemAvailable and describes the Linux/WSL-visible kernel. It is not
the Windows VMM process working set or the configured Windows .wslconfig maximum.
Runtime cgroup v2 memory is a separate observation: current leaf usage and the
minimum finite memory.max over its ancestors, null limit for verified unlimited.
Unavailable controllers/files remain unavailable. No cgroup-v1 limit is invented.
Linux CPU excludes guest counters already included in user/nice; Windows CPU
includes idle within kernel time and declines whole-machine reporting on multiple
processor groups rather than publishing a partial value.

Storage reports filesystem capacity/used/identity-available bytes for the Host
state volume and managed Projects filesystem. It does not recursively scan
Projects or present filesystem usage as a Project directory's byte count.
GPU UUID, name, utilization, memory used/total, temperature and power come from
read-only NVIDIA queries. MiB values are converted to bytes. Individual N/A values
remain unknown. A shared GPU UUID identifies the same physical device across OS
views; the observations need not have identical times. Global VRAM usage is never
attributed to a Notebook. Process attribution explicitly remains NOT_SUPPORTED.

The diagnostic executable comes only from the Windows system directory or fixed
Linux `/usr/lib/wsl/lib`/`/usr/bin` paths, with its protected directory as cwd and
fixed query arguments. No shell is used. It receives only LC_ALL and, on Windows,
SystemRoot/ProgramFiles; worker/Runtime credentials and Python/loader injection
variables are not inherited. Stdin is closed, stderr is discarded, stdout is
bounded to 64 KiB and at most 64 unique GPUs, and the child is killed/reaped on
timeout (two seconds), invalid output or failure. OS filesystem reads use a
blocking task and bounded text; storage syscall stalls do not hold Runtime admission.
Explicit shutdown joins collector work under a four-second budget and reports
pending cleanup rather than silently abandoning owned work. Last owner drop
closes the collector after current bounded diagnostic work settles.

## 29. Local MCP transport and shared Runtime admission

The Linux Runtime optionally binds `COKERNEL_RUNTIME_SOCKET` when its owned
stdio bridge starts. The installer must supply an absolute path in an existing
private control-identity directory; there is no public/default socket fallback.
Both ends validate canonical parent paths, current UID ownership and no group or
other permissions. The socket is mode 0600 and both peers verify kernel UID
credentials. A private, no-follow lock prevents competing listener replacement.
Only an owned Unix socket that refuses connection may be recovered after a crash;
active listeners, symlinks and unrelated files are preserved and rejected.

The API admits at most eight live connections, each with one framed request.
Requests are limited to 1 MiB and five seconds to arrive; responses to 8 MiB and
five seconds to write. Reply waiting is capped at 120 seconds. IDs are nonempty,
bounded to 128 bytes and contain no control characters. A permitted method goes
through the same RuntimeHandle as Human stdio requests, with origin fixed to MCP.
The allowlist contains domain reads, primary Session lifecycle/execution/inspection,
operation status and cached Linux metrics. The release authoring increment also
admits `projects.create`, `packages.add/sync`, and `notebooks.create/open/apply_edits/save`.
It excludes whole-Runtime shutdown, credential/tunnel configuration, Project
removal and arbitrary document import/filesystem operations. Transport loss releases only
the reply/connection, never cancels or replays accepted work. Existing domain
Project ownership, revision and per-Notebook primary/FIFO checks remain authoritative.

`cokernel-mcp serve <runtime-socket> <private-bearer-file> <loopback-port>` connects
to an existing compatible Runtime before reporting `mcp.ready`. It never starts
another Runtime or worker. The service uses the official Rust MCP SDK with
Streamable HTTP, JSON replies, stateless transport and `/mcp` only. Loopback
binding is mandatory. Exact bound/localhost Host and Origin values are allowed;
absent Origin supports non-browser clients. Remote tunnel routing must provide
explicit approved authority/origin configuration before remote access is enabled.

Every HTTP method requires exactly one valid Bearer header. The bearer is 32 OS
random bytes encoded as 64 hex characters, stored in a private regular mode-0600
file under a canonical private control directory. Existing unsafe/invalid files
are rejected rather than silently rotated. Secrets do not appear in readiness,
configuration arguments or ordinary logs. Restart reloads the same bearer.

HTTP admission is capped at 16 live connections, 64 headers, a 16 KiB header
buffer, five seconds for headers, a 256 KiB request body and 130 seconds per
connection. Keep-alive is disabled. Runtime RPCs have a three-second connect and
125-second exchange budget, verify protocol/correlation/result shape, and never
automatically retry. POST bodies must be single JSON-RPC objects; string IDs are
nonempty, at most 128 bytes, without controls; integer IDs stay within the exact
JSON integer range. This bounds correlation echo as well as request content.
Both success and error tool content are capped at 128 KiB of JSON text, with a
second encoded CallToolResult limit of 256 KiB minus 4 KiB envelope allowance.
Oversized values produce CK-MCP-LIMIT; transport uncertainty produces
CK-MCP-OUTCOME-UNKNOWN and is never replayed automatically.

## 30. Native MCP tools and bounded notebook view

`cokernel-mcp::contracts` is the single declarative source for tool schemas and
input validation. `protocol/mcp/tools.json` snapshots the registered
names, descriptions, schemas and annotations. Unknown fields, missing fields,
invalid UUIDs, non-integer/out-of-range revisions and expression variable names
are rejected before Runtime dispatch. Domain ownership/revision checks remain
in Runtime. Caller `code`, `source`, `origin`, executable paths and PIDs are not
accepted by execute/ensure. Remote clients never choose execution origin.

All fourteen minimum tools in MCP_TUNNEL section 5 are registered, plus the
read-only `get_operation(project_id, operation_id)` for asynchronous execution
and document-persistence status. Eight authoring tools bring the total to 23:
`create_project`, `create_notebook`, `open_notebook`, `insert_code_cell`,
`set_cell_source`, `save_notebook`, `add_packages`, and `sync_environment`. Edits are translated into
the existing document edit operations and require `expected_revision`; source
is limited to 16 KiB UTF-8, package requests to 32 bounded strings without option
prefixes or control characters. Creation/package operations return receipts to
poll; save confirms atomic persistence of the supplied revision. Existing live
Sessions survive package mutation and require explicit restart to adopt it.
Every scoped tool takes `project_id`; Session
get/interrupt/stop/inspection additionally takes `session_id`; ensure/restart
takes `notebook_id`; execute takes both plus `cell_id` and `expected_revision`.
Restart explicitly loses volatile memory. `get_resources` reports cached Linux
observations, not the Windows aggregate. Runtime status performs the compatible
bridge handshake and never starts a Session.

Variable names are one Unicode XID identifier, excluding Python hard keywords,
up to 256 characters. The worker independently validates against its Python
version before direct namespace lookup and exact-type serialization. Unicode
versions may differ; worker rejection never causes expression evaluation.
Cell IDs are 1..64 ASCII letters/digits/underscore/hyphen. Revisions are exact
nonnegative JSON integers no greater than 2^53-1. All tool objects are closed.

`list_notebooks` returns paginated paths (offset default 0, limit default 32,
maximum 128) and `open_documents` for those paths in the same Project. Existing
open documents provide stable IDs/revisions. Closed paths are not silently
opened or assigned new live identities by a read-only tool; Desktop or the
explicit mutating `open_notebook` tool opens them.

`read_notebook` maps to shared `notebooks.read_cells`, executed by the document
owner. It requires Project/Notebook IDs and supports optional expected_revision,
cell offset (default 0, maximum 10,000), limit (default 16, maximum 32), and
include_outputs (default false). It returns document/state/revision, total_cells,
offset, next_offset and cells with index, cell_id/type, source, source_bytes,
source_truncated, execution_count, outputs, outputs_included/truncated and
output_bytes. Pass the returned revision to protect subsequent pages.

The view includes at most 64 KiB of serialized cell entries per page. Each source
preview is capped at an 8 KiB UTF-8 boundary; each optional output array is either
included whole within 8 KiB or omitted with an explicit truncation flag. Metadata,
attachments and extension fields are not implicitly copied. Stored notebook
content is never truncated or changed by viewing. Raw revision-checked document
chunks remain available through the existing domain read_chunk contract.

## 31. Optional Runtime-owned MCP supervision

Runtime accepts trusted startup configuration `COKERNEL_MCP_EXECUTABLE`,
`COKERNEL_MCP_BEARER_FILE`, `COKERNEL_RUNTIME_SOCKET` and optional
`COKERNEL_MCP_PORT` (default 0). Paths are absolute with canonical parents;
the executable is a regular, executable, root/control-owned file without
group/other write permission; socket/bearer parents are private to the control
identity. The installer must protect the executable's parent tree. Domain/MCP
callers cannot supply these paths. Missing all MCP settings disables supervision.
Invalid settings or optional Unix API bind failure report MCP failure while local
bridge/domain execution continues. A startup configuration error requires repair
and Runtime restart; an exhausted process recovery circuit supports MCP-only restart.

An independent task owns MCP validation, launch, readiness, health, retry and reap.
`runtime.get_mcp_status({})` returns state, generation, optional PID/address,
consecutive_failures and at most eight failure entries (static code, UTC timestamp,
exit code/signal). It never returns raw subprocess output, paths or credentials.
States are DISABLED/STARTING/READY/BACKOFF/FAILED/STOPPING/STOPPED. Changes publish
`mcp.state_changed`; `runtime.get_snapshot` includes the same `mcp` structure.
The existing MCP `get_runtime_status` adds this structure to its bridge handshake.
Host supports both the read and `mcp.restart({})`; the latter returns
`{accepted:true}` and coalesces restart requests. The MCP Unix API permits only the
status read, never self-restart. These operations never restart Notebook Sessions.

The child has a private process group, cleared environment (fixed PATH/LC_ALL),
null stdin/stderr, protected executable-directory cwd and Linux parent-death kill.
Only credential file paths, never bearer values, appear in launch arguments.
Readiness requires a <=4096-byte JSON line within 10 seconds, matching localhost
address/port, `/mcp` and authenticated=true, plus a <=1-second HTTP 401 health probe.
Unexpected later stdout/EOF fails the service. Health probes run every two seconds;
three failures trigger recovery. The MCP binary exits when its listener task fails.
The first successfully bound ephemeral port is reused for recovery within this
Runtime lifetime, and the bearer file is retained. No accepted domain work is replayed.

Recovery allows an initial attempt plus three retries, delayed 250 ms, 1 second
and 4 seconds. Four consecutive failures open the circuit (FAILED). Sixty seconds
of healthy service resets the failure allowance on the next failure. Explicit
MCP restart also resets the allowance, retaining failure history. Cleanup sends
TERM, allows one second, then kills the group before reaping the leader. Readiness
and backoff waits are cancellable. A successful explicit Runtime shutdown joins
MCP cleanup before acknowledgement; cleanup uncertainty refuses success and remains
owned. Earlier document/package shutdown refusal leaves MCP running. Startup/failure
history here is bounded live state, not durable Session FailureRecord storage.

## 32. Windows tunnel credential configuration

Only authenticated current-user Host IPC exposes:

- `remote_access.configure({tunnel_id,runtime_key})`: validate and replace one
  paired credential atomically through Windows Credential Manager.
- `remote_access.get_configuration({})`: return only configuration presence and ID.
- `remote_access.clear_configuration({})`: remove that Host instance's credential;
  absent storage is already clear.

All successes return `{configured:boolean,tunnel_id:string|null}`. The schema is
closed. Tunnel IDs are 8..128 ASCII alphanumeric/underscore/hyphen bytes beginning
with `tunnel_`; keys are opaque 16..2048 printable non-space ASCII bytes. Neither
field accepts paths, executable arguments or control-plane endpoints. Key validity
and tunnel permissions are verified during the subsequent actual connection;
successful storage does not claim CONNECTED. The Host capability is
`windows-secret-store`. MCP and Runtime Unix APIs expose none of these methods.

The target is `CoKernel/v1/<SHA256-of-canonical-state-path-UTF16LE>/tunnel`.
Credential blobs use `CKTC`, version byte 1, two-byte big-endian ID length, ID and
remaining opaque key bytes, bounded by the Windows 2560-byte blob limit. Corrupt
or unknown-version state is rejected without automatic deletion or replacement.
Win32 errors become static CK-SEC-CREDENTIAL-* errors without input/OS diagnostic
echo. CRED_TYPE_GENERIC and CRED_PERSIST_LOCAL_MACHINE scope persistence to the
current Windows user on this PC.

A dedicated blocking owner serializes eight queued operations plus one running
operation. Credential calls do not hold Runtime/Host control admission. Accepted
writes remain owned after a reply/client disappears; callers refresh configuration
before retrying uncertain work. Dropping the last owner handle drains accepted
work. It does not retain decrypted credentials between operations. Typed key and
encoded blob buffers use zeroizing ownership; Win32 read buffers are wiped before
CredFree. This is best-effort application-buffer hygiene, not a guarantee that
transport/JSON/OS copies never existed in process memory.

These configuration methods are serialized by the Host Remote Access owner in
section 34. Configuration alone does not enable Remote Access or start Runtime.
Replacement while enabled invalidates the applied credential revision; clear
first persists disabled intent and confirms live tunnel cleanup before deleting
the Windows credential. No secret-read RPC is exposed.

## 33. Runtime-owned tunnel component

Trusted bootstrap may set `COKERNEL_TUNNEL_EXECUTABLE` and
`COKERNEL_TUNNEL_STATE_DIR`, with `COKERNEL_MCP_BEARER_FILE` from MCP configuration.
Absent tunnel configuration leaves Remote Access disabled. Invalid optional
configuration degrades only Remote Access. Paths must be canonical and private;
bearer reads require a regular non-symlink owner-only control-owned file with 64 hex bytes.
Each attempt creates a private unique directory/socket. Production RPC callers
cannot set executable paths, provider URL, environment, flags or health listeners.

Only the trusted stdio bridge currently accepts `tunnel.start({tunnel_id,runtime_key})`,
`tunnel.stop({})`, `tunnel.restart({})`. Start/stop/restart return
`{accepted:true,epoch:integer}` after nonblocking admission, not connection success.
Repeated start with identical credentials/stop with no desired credentials are
idempotent. Restart is explicit and requires an enabled configuration. Inputs are
closed/bounded as in section 32; errors use fixed CK-TUN-* codes and summaries.
These controls are excluded from the MCP Unix API, and domain admission rejects
non-Human-origin tunnel controls. This is an internal Host integration surface;
Desktop clients must not send credentials directly to Runtime.

`runtime.get_tunnel_status({})` and the `tunnel` member of `runtime.get_snapshot`
contain state, desired_enabled, applied_epoch, generation, mcp_generation, PID,
consecutive_failures and at most eight sanitized failures. `tunnel.state_changed`
publishes only changes. `applied_epoch` refers to the owned incarnation while a
child exists, or the reconciled intent when none exists; it is not a connection
success assertion. The normal lifecycle and health policy are MCP_TUNNEL section 21.

Launch clears the environment, supplies provider/local-MCP keys only to the tunnel
environment, uses env references in argv, disables user-profile inheritance and
discards raw output. Owned key/bearer buffers are zeroized; serialized bridge/OS
copies are not claimed to be fully erased. The tunnel process has its own group
and parent-death signal. Explicit Runtime shutdown completes its cleanup before
MCP cleanup and acknowledgement. Bridge loss also drops its desired credentials
and joins cleanup. Notebook Sessions are not owned by this supervisor.

The Windows owner described in section 34 is the caller of this private handoff.

## 34. Host Remote Access owner

Current-user Named Pipe exposes `remote_access.start({})`, `stop({})`, `restart({})`
and `get_status({})`, alongside the section 32 configuration methods. Inputs are
closed. No client-supplied bridge method, path, URL or launch environment is accepted.
Start requires a stored credential. Stop preserves the credential. Clear disables
intent first, confirms DISABLED/no live child on an available bridge, then deletes
the credential. If cleanup is uncertain it returns CK-REMOTE-CLEANUP and preserves
the credential; disabled intent remains durable and reconciliation continues.

Remote intent is separate from Runtime desired state. A stopped Runtime is never
implicitly started by remote enable. The Host stores only
`{schema:1,enabled:boolean}` in atomic `remote-access.json` under its locked state
directory. Missing state means disabled; malformed/oversized/unsupported state
reports ERROR until explicit control/repair. The file is <=1024 bytes; it holds no
credential or observed connection state. Credential truth remains Windows storage.

One bounded owner serializes eight waiting remote mutations plus one active job.
Accepted jobs outlive their callers. Blocking OS storage is off the async executor;
remote control waits never hold ordinary Host domain admission. Host exit drains
accepted remote jobs before releasing the state-directory lock. Remote intent is
preserved across Host exit, like Runtime intent; explicit stop persists disabled.

The owner compares `(Runtime generation, credential revision, effective enabled)`
with the last applied target. A changed target loads the current Windows credential
once and sends it through the owned framed bridge; an unchanged target reads only
sanitized status. Configuration changes invalidate the revision, and new Runtime
generations load the latest credential rather than replaying a stale cached key.
Identical enable does not bypass either recovery circuit. Explicit restart reloads
configuration, verifies application, then requests a new tunnel incarnation.

Status contains `desired_enabled` (null during initial LOADING), state, Runtime and
tunnel generations, bounded failure count and fixed error code. With no Runtime,
enabled intent displays WAITING_FOR_RUNTIME. A still-owned stopping bridge cannot
be reported as DISABLED. Observations are tied to the current bridge generation
and runnable state. `remote_access.state_changed` emits only changed safe metadata.
No arbitrary Runtime error/result text from credential operations is forwarded.
Invalid status, false DISABLED with a PID or CONNECTED without a PID is rejected.

Bridge requests are bounded and correlated. Handoff frames and extracted owned key
buffers are zeroized after encoding/use, with the existing transport/JSON/OS-copy
limitations. Each remote bridge call has a three-second observation timeout;
accepted work can still finish, so reconciliation uses idempotent start/stop and
epoch checks. Uncertain handoff/status failures allow three delayed retries, then
report ERROR until a new target or explicit reconnect. This does not restart local
Runtime or Sessions. Whole-worker identity isolation and actual external-client
acceptance remain mandatory separate gates.

## 35. Workload subprocess environment

The Runtime treats uv dependency operations (including build hooks) and uv worker
startup as workload execution. Both use one cleared environment policy and an
explicit Project cwd/HOME. UvInvocation carries its Project root as a typed path;
launch context is not reconstructed by recognizing argument patterns. Arguments
remain structured and are not passed to a shell.

Trusted PATH and configured uv cache/Python-install locations are the only
inherited Linux tool-location settings. Locale, progress, IPython and XDG paths
are supplied explicitly as described in SECURITY_OBSERVABILITY section 7.
No arbitrary UV_*, PYTHON*, product secret, control profile or agent variable is
forwarded. User dependency configuration remains in the Project. This is a
subprocess environment contract, not the managed Linux UID/file permission gate.

## 36. Managed workload identity boundary

Runtime startup parses trusted COKERNEL_WORKLOAD_UID/GID into one typed policy and
passes that same policy to the Project uv runner and Session Supervisor. Validation
and transition rules are SECURITY_OBSERVABILITY section 4 and decision D-027.
Invalid managed startup returns CK-SEC-WORKLOAD before serving requests. Launch
permission failures flow through existing structured Project/Session error paths;
they do not trigger an identity fallback or a second execution runtime.

Session lifecycle and per-Notebook primary identity are unchanged. The Session
endpoint validates the configured workload peer UID in addition to authenticated
kernel PID/descendant lineage. Runtime still owns process groups and interrupt,
stop, cancellation and reap. Package build hooks and worker descendants inherit
the workload credentials and no_new_privs, not Runtime's control privileges.
Managed UID proof does not establish installed WSL automount/interop restrictions
or actual Desktop/Codex/Web ChatGPT acceptance.

## 37. Shareable Runtime diagnostic snapshot

`runtime.get_diagnostics` accepts exactly `{}` through the Host/Runtime bridge.
Unknown parameters return CK-DIAG-INPUT without echoing values. Host advertises
`runtime-diagnostics-forwarding`; a compatible Linux Runtime advertises
`sanitized-diagnostics`. No new MCP tool or Unix API privilege is introduced.

Schema 1 contains product_version, captured_at, scope, exclusions, sessions, mcp
and tunnel. Session diagnostics contain managed_identity_configured, total,
omitted and at most 128 items with opaque Session ID, state, worker/environment
generation and optional latest typed failure summary. This flag describes the
configured launch policy, not successful completion of installed-system security
acceptance. The bounded subset has no ordering/pagination guarantee.

MCP/Tunnel diagnostics use enum states, numeric generations/counts, process-present
booleans and at most eight failure summaries (newest first). Failures use a closed
kind enum, validated optional timestamp and numeric exit/signal. Unknown textual
codes/timestamps are not echoed. Tunnel includes desired_enabled/mcp_generation;
addresses, PIDs, credentials and free-form metadata are excluded.

These are current component observations, not an atomic transaction across all
components. The operation uses bounded in-memory observations and never queues
behind document I/O or loads raw log/history files. Serialization is bounded at
128 KiB; CK-DIAG-LIMIT/ENCODING are fixed errors. Future ZIP collection must use
this privacy contract and declare any additional collection scope explicitly.

## 38. Human remote credential-entry adapter

Windows `cokernel-host remote configure` connects to the existing current-user
Host pipe before opening native credential input. Extra CLI arguments are rejected
without echo, including for status/start/stop/restart/clear. Input is collected on
a client blocking thread, never the Host request owner. Cancel performs no RPC.
The native adapter's OS-specific length boundary is documented in MCP_TUNNEL.

`PipeClient::configure_remote` serializes borrowed typed credential fields directly
into a zeroizing transport frame, without an intermediate generic JSON request.
It shares normal request correlation, cancellation poisoning and peer-identity
validation. Only an exact configured=true/matching-tunnel-ID receipt is returned;
unknown fields, false acknowledgements, transport failures and arbitrary server
errors produce a fixed setup error without copying response text. Uncertain
completion requires observing configuration rather than automatically replaying
a mutation. Windows credential blobs and UTF-16/byte buffers are wiped on release.
Read/write frame buffers are also wiped on drop. This does not claim to erase
all OS, serde or allocator copies.

`remote status` returns existing configuration metadata and remote status.
`remote start|stop|restart|clear` call the existing corresponding owner operations.
No operation starts a new Host or overrides Runtime's explicit stopped state.
The client commands add no new IPC/MCP method or persistence owner.
