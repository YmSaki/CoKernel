# CoKernel v1 Architecture

Status: **implementation baseline**

## 1. Architectural goal

CoKernel v1 is a Windows desktop product whose compute plane runs inside a managed WSL Linux environment.

The main execution path is:

```text
Windows Desktop
   -> Windows Host
      -> persistent WSL control bridge
         -> CoKernel Runtime
            -> Project/uv environment
               -> supervised Python worker
                  -> IPython execution semantics
                     -> .ipynb outputs/state
```

AI access is a parallel client path into the same domain/runtime:

```text
ChatGPT / AI client
   -> Secure Tunnel
      -> CoKernel MCP
         -> CoKernel Runtime API
            -> same Project / Notebook / Execution Session
```

The runtime is built around Project, NotebookDocument, and ExecutionSession rather than exposing internal process identifiers to ordinary users.

## 2. Technology choices

### 2.1 Core/control implementation: Rust

Use Rust for:

- Windows Host;
- WSL Runtime daemon/CLI;
- Session Supervisor;
- Project/Notebook domain services;
- IPC/protocol implementation;
- MCP service where practical;
- resource/diagnostic orchestration;
- installer/bootstrap helper logic that benefits from a native binary.

Rationale:

- strong process/lifecycle control;
- good Windows and Linux support;
- one shared domain/protocol model across host and runtime;
- static/self-contained deployment options;
- memory safety around long-running supervisors;
- Tokio/Serde ecosystem fits concurrent process/IPC work.

### 2.2 Execution worker: Python

Use a small Python worker launched by the Project's selected Python interpreter.

The worker exists because Python/IPython execution semantics must run inside the Project Python process. It is not the system control plane.

Worker responsibilities:

- initialize an IPython execution shell/session;
- maintain the persistent namespace;
- execute code-cell requests;
- capture stdout/stderr/results/rich display/errors;
- support interrupt cooperation;
- expose constrained variable inspection primitives;
- send structured events to the Rust Session Supervisor.

### 2.3 Project/environment management: uv

`uv` is the dependency/environment implementation for Python Projects.

CoKernel delegates:

- Python selection/install where supported;
- project initialization;
- dependency resolution;
- lockfile maintenance;
- virtual environment creation/sync;
- package download/cache/link behavior.

CoKernel does not duplicate uv's resolver/cache model.

### 2.4 Desktop UI: Tauri + TypeScript frontend

Use Tauri v2 for the Windows desktop shell with a small TypeScript frontend. Initial frontend recommendation: React + CodeMirror 6.

Reasons:

- native Windows executable with WebView2 already present on modern Windows;
- simple tray/window integration;
- Rust command/event integration;
- CodeMirror provides practical cell editing without building a text editor from scratch;
- visual polish can remain modest while interaction quality stays high.

UI framework choice is not a domain contract; Tauri commands must call Host IPC rather than own runtime logic.

## 3. Windows control plane

```text
CoKernel Desktop.exe
        |
        | Named Pipe RPC/events
        v
CoKernel Host.exe
        |
        | long-lived wsl.exe bridge
        v
Managed WSL
```

### Windows Host responsibilities

- desired runtime state;
- WSL lifecycle ownership;
- auto-start;
- secret storage/brokering;
- runtime bridge lifecycle;
- update/repair orchestration;
- Windows-side metrics;
- notifications;
- local IPC for Desktop.

The Desktop process may close/restart without terminating Sessions merely because the UI exited.

## 4. WSL compute plane

A fresh dedicated distro is managed by CoKernel.

Recommended v1 Linux identities:

```text
cokernel-runtime   product runtime/control process identity
cokernel-user      Project files and user execution identity
cokernel-tunnel    tunnel process identity where practical
```

Exact user/group mechanics may be adjusted during bootstrap implementation, but the trust goal is fixed: notebook code must not inherit external tunnel/Windows secrets.

Suggested layout:

```text
/opt/cokernel/
  bin/
  runtime/
  worker/

/etc/cokernel/
  runtime.toml

/var/lib/cokernel/
  state/
  logs/
  diagnostics/
  sockets/

/home/cokernel-user/projects/
  <project-id-or-name>/

/home/cokernel-user/.cache/uv/
```

Project roots and uv cache should live on the same WSL-native filesystem when possible to preserve uv's efficient linking/copy-on-write behavior.

## 5. Host <-> WSL bridge

The Windows Host starts a long-lived WSL process such as:

```text
wsl.exe -d CoKernelV1 --exec /opt/cokernel/bin/cokernel-runtime bridge --stdio
```

This process serves two purposes:

1. stable versioned control channel between Windows Host and Linux Runtime;
2. explicit Windows-owned WSL lifetime while CoKernel desired state is RUNNING.

Transport:

- framed JSON messages over stdin/stdout;
- protocol version on handshake;
- request IDs;
- asynchronous event stream;
- no secret values in ordinary logs.

The bridge is not a shell-command scraping interface.

## 6. Runtime daemon/services

Inside WSL, `cokernel-runtime` owns:

- Project registry;
- uv Project Environment Manager;
- notebook document service;
- Session Supervisor;
- resource/diagnostic collection;
- local runtime API socket;
- lifecycle of CoKernel MCP and tunnel processes.

The daemon uses a Unix domain socket for local Linux-side clients/services in addition to the Windows bridge.

## 7. Execution worker architecture

Each live ExecutionSession has one worker process.

```text
Session Supervisor (Rust)
    |
    | Unix domain socket protocol
    v
Project Python worker
    |
    v
IPython
```

The supervisor launches the worker using the Project's resolved Python environment.

Protocol is **not** carried on worker stdout/stderr because native/Python libraries can write directly to those file descriptors. The supervisor captures stdout/stderr as diagnostic streams while structured worker control/events use a private Unix socket.

### Execution serialization

Per Session:

```text
one FIFO queue
one executing user-code operation at a time
```

Across Sessions:

```text
independent queues
concurrent worker processes
```

This gives deterministic shared state to Human + AI while preserving parallelism between notebooks.

## 8. Worker tooling injection

The worker must run with the Project interpreter while keeping CoKernel runtime tooling separate from user dependency truth.

Implementation shall begin with a dedicated technical spike comparing supported uv mechanisms for injecting/launching the CoKernel worker/IPython tooling without mutating user-declared dependencies unnecessarily.

Acceptance criteria for the chosen method:

- Project packages are importable normally;
- `sys.executable` corresponds to the intended Project Python environment/interpreter semantics;
- `pyproject.toml`/`uv.lock` are not polluted with CoKernel runtime dependencies unless explicitly required by the selected design;
- worker/runtime package versions are controlled by CoKernel;
- `uv sync` does not leave the Project in a broken worker state;
- multiple Python versions supported by v1 behave deterministically.

This spike is a Phase-0 implementation gate because it affects the worker launch contract.

## 9. Notebook document service

The Rust Runtime reads/writes nbformat v4 JSON directly with Serde-based structures while preserving unknown metadata.

Responsibilities:

- parse/validate/import `.ipynb`;
- maintain document revisions;
- apply cell edits;
- apply execution outputs;
- atomic writes;
- external-change detection;
- emit document update events to Desktop/MCP clients.

The NotebookDocument service, not the worker, owns file writes.

This prevents competing human/AI/worker file writes and makes revision/conflict handling deterministic.

## 10. Windows notebook import path

Windows drives are not permanently mounted into the workload environment.

Import flow:

```text
Windows file picker
 -> Desktop/Host opens selected file
 -> Host streams validated bytes over runtime bridge
 -> Runtime writes to requested Project-relative path atomically
```

Transfer is streamed/chunked rather than assuming a small notebook.

The Runtime validates path containment and file format before replacing/creating Project state.

## 11. MCP architecture

Run a CoKernel-native MCP service inside the managed Linux runtime.

```text
cokernel-mcp
   -> local authenticated Runtime API / domain service
```

MCP tools map to CoKernel domain operations, not to a separate notebook execution engine.

The MCP service never owns a Python Session; it references the same Sessions used by Desktop.

HTTP transport binds only to a private/local interface expected by the tunnel and local diagnostics.

## 12. Secure tunnel architecture

The managed tunnel client runs as a separate supervised process in WSL.

```text
Internet/OpenAI control plane
        ^ outbound
        |
secure tunnel client
        |
        v
CoKernel MCP local endpoint
```

Rules:

- tunnel starts after MCP is ready;
- tunnel failure does not stop local runtime/Sessions;
- external credentials originate from Windows secret storage and are handed to the tunnel without making them readable by notebook workers;
- local MCP authentication remains enabled even though it is private.

## 13. Containerization decision

The primary Notebook execution path does not require an additional container boundary because the managed WSL environment is already the Linux compute boundary and Project-level execution needs efficient access to uv environments and GPU processes.

A component may still be containerized later if it produces a concrete security/deployment benefit, but Docker/Compose is not a fundamental v1 runtime dependency.

## 14. Networking

Normal notebook execution does not require a Windows localhost web service.

Primary local paths are:

```text
Desktop -> Windows Named Pipe -> Host -> WSL stdio bridge -> Runtime
Runtime -> Unix sockets -> Worker/MCP local integration
```

The MCP/tunnel endpoint is the only HTTP-style service required by the core v1 design.

No public inbound listener is created by default.

## 15. Parallel execution model

```text
Project A
  Notebook train.ipynb    -> Session A -> Worker A
  Notebook analysis.ipynb -> Session B -> Worker B

Project B
  Notebook test.ipynb     -> Session C -> Worker C
```

A/B/C may run concurrently.

Within Session A, Human and AI requests are serialized in A's queue.

Resource admission initially remains conservative/simple rather than implementing a full scheduler. The runtime must know current machine metrics and fail/warn clearly under resource pressure. Later Fabric scheduling can reuse Session resource metadata.

## 16. Persistence model

Durable:

- Project files;
- `pyproject.toml`/`uv.lock`;
- `.ipynb` documents and stored outputs;
- product settings;
- bounded operation/failure logs;
- encrypted external credential records on Windows.

Volatile:

- Python namespace/object memory;
- live Session processes;
- MCP connections;
- tunnel connection state.

After Windows/runtime restart, Projects and notebooks return; volatile Session memory is reported as ended, not reconstructed by implication.

## 17. Build/repository model

Use a Cargo workspace for Rust binaries/libraries and one Python worker package directory.

Suggested roots:

```text
crates/
  cokernel-domain/
  cokernel-protocol/
  cokernel-host/
  cokernel-runtime/
  cokernel-mcp/
  cokernel-bootstrap/

worker/
  cokernel_worker/
  pyproject.toml

desktop/
  src-tauri/
  src/

installer/
  windows/

docs/v1/
```

## 18. Architectural boundaries that must not be bypassed

1. Desktop does not directly invoke uv/WSL/workers as its normal path; it talks to Host.
2. Host does not parse ad-hoc shell text as the runtime API; it uses the versioned bridge.
3. MCP does not create a separate execution universe; it calls the same runtime/session domain service.
4. Worker does not write `.ipynb` files directly; it emits structured execution events.
5. Project dependency state is changed through uv integration, not ad-hoc pip mutation.
6. Session processes never receive external tunnel/Windows secrets.
7. One Session never executes two user-code operations concurrently.