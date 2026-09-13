# CoKernel v1 Conceptual Design

Status: **implementation baseline**

## 1. Core concept

CoKernel v1 turns a Windows NVIDIA GPU PC into a managed Linux compute environment whose primary user model is:

```text
Machine Runtime
  -> Project
     -> Project Environment (uv)
     -> NotebookDocument (.ipynb)
        -> ExecutionSession
```

Human UI and AI/MCP operate on these same domain objects.

The user should be able to think:

```text
Create Project
 -> Add packages
 -> Create/Import notebook
 -> Run cells
```

without having to choose internal Python process identifiers or manually administer the Linux runtime.

## 2. Windows and Linux responsibilities

### Windows

Windows is the product/control-plane surface.

It owns:

- Desktop/tray UI;
- installation and update;
- desired runtime state;
- WSL lifetime ownership;
- Windows-side metrics;
- external credential protection;
- notifications;
- Windows file import initiation.

### Managed WSL

WSL is the Linux compute plane.

It owns:

- uv Projects/environments;
- Project files/notebooks;
- supervised Python/IPython Sessions;
- notebook document service;
- GPU workload execution;
- CoKernel MCP;
- tunnel client lifecycle;
- Linux-side metrics/diagnostics.

Users normally do not administer WSL directly.

## 3. Project

Project is the dependency/environment boundary.

```text
Project
├─ pyproject.toml
├─ uv.lock
├─ .venv / uv-managed environment
├─ source/data
└─ *.ipynb
```

CoKernel uses uv rather than creating its own Python dependency resolver/cache system.

Logical isolation is per Project, while uv's supported cache/link mechanisms are used to reduce unnecessary physical package duplication.

## 4. Notebook document

A NotebookDocument is a durable standard `.ipynb` file.

It stores cells, Markdown, outputs, attachments, metadata, and execution counts according to supported nbformat semantics.

A NotebookDocument is not the Python process and does not contain arbitrary live objects from RAM.

Existing `.ipynb` files can be imported/uploaded from Windows into a Project through a controlled transfer path.

## 5. Execution Session

An ExecutionSession is the live volatile compute state for a notebook.

```text
NotebookDocument
      |
      v
ExecutionSession
      |
      v
Project Python process
      |
      v
IPython namespace
```

It owns variables, imports, loaded models, framework/GPU runtime state, and the execution queue.

By default one notebook has one primary live Session so Human and AI have one shared namespace.

## 6. Parallel execution

Different Sessions are independent processes and may execute concurrently.

```text
train.ipynb    -> Session A
analysis.ipynb -> Session B
benchmark.ipynb-> Session C
```

Within each Session, execution is serialized to preserve deterministic interactive state.

Thus CoKernel provides parallelism **between** Sessions and deterministic ordering **inside** a Session.

## 7. Interactive Python semantics

The worker uses IPython as a library/runtime to provide Python notebook behavior such as:

- persistent namespace;
- final-expression results;
- stdout/stderr;
- exceptions/tracebacks;
- rich MIME display;
- top-level async;
- commonly used IPython interactive behavior.

The control plane itself is not required to be Python.

## 8. Supervision

Each Session worker is a child process supervised by CoKernel Runtime.

A Python/native/GPU library crash should affect that Session, not terminate the control plane or unrelated Sessions.

CoKernel captures available evidence before recovery:

- exit/signal;
- last operation/cell;
- stderr tail;
- memory state;
- GPU state;
- OOM evidence;
- surrounding Runtime events.

The product reports what happened as far as evidence allows rather than only reporting an internal restart state.

## 9. Environment changes

Changing Project dependencies through uv does not silently destroy existing Session memory.

Existing Sessions keep running but become `STALE_ENVIRONMENT`; restarting them intentionally starts a new worker on the current Project environment.

## 10. Notebook persistence

The Notebook Document Service is the authority for CoKernel-mediated `.ipynb` writes.

Human edits, AI edits, and execution outputs are revisioned and applied through this service. Workers only emit structured execution events.

This provides one place for:

- atomic save;
- revision conflicts;
- external modification detection;
- output persistence;
- Human/AI synchronization.

## 11. AI/MCP

AI uses a CoKernel-native MCP service.

MCP addresses Projects, notebooks, Sessions, variables, and resources. It calls the same Runtime domain service as local clients.

Joining a notebook means joining its existing primary Session when present, not creating hidden compute state.

Human and AI execution requests use the same per-Session FIFO queue.

## 12. Secure remote access

External AI reaches the local authenticated MCP endpoint through a managed outbound secure tunnel.

Tunnel failure degrades remote access only; local Project/Notebook/Session execution remains available.

External credentials are protected on Windows and not exposed to notebook workers.

## 13. Application UX

Normal UI centers on:

```text
Projects
Packages
Notebooks
Sessions
Resources
Remote AI Access
Logs/Diagnostics
```

Internal process/protocol details are visible only where useful for diagnostics/developer mode.

The primary design criterion is that operations and failures are understandable. Visual ornamentation is secondary.

## 14. Persistence boundary

Durable:

- Projects;
- dependency declarations/locks;
- notebook documents/outputs;
- settings and bounded diagnostics.

Volatile:

- Python namespaces;
- worker processes;
- live MCP/tunnel connections.

Windows/runtime restart does not imply that volatile Python memory was preserved.

## 15. v1 installation boundary

v1 is installed as a fresh managed runtime. Existing experimental runtime state is not an in-place compatibility contract.

Released installer explicitly confirms destructive removal of a detected legacy distro and may offer export of wanted user files before reset.

Routine v1-to-v1 updates, by contrast, are designed to preserve Projects/notebooks and managed state.

## 16. Future direction

The v1 Runtime API/domain model is intentionally independent from the visible Desktop UI so a future remote CoKernel Node can expose the same:

```text
Project
Environment
Notebook
ExecutionSession
Resources
```

That allows later multi-PC/Fabric work without redesigning the local notebook execution model.