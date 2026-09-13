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

Recommended pipe name:

```text
\\.\pipe\cokernel-v1
```

The Host validates the connecting Windows user/ACL.

### Runtime methods

```text
runtime.get_status
runtime.start
runtime.stop
runtime.restart
runtime.get_versions
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

## 11. Error code families

```text
CK-INST-*  install/bootstrap
CK-HOST-*  Windows Host/bridge
CK-RUN-*   WSL Runtime
CK-PROJ-*  Project
CK-UV-*    environment/package
CK-NB-*    notebook document/import
CK-SES-*   Session/supervision
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