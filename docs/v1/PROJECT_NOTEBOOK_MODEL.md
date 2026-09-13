# CoKernel v1 Project and Notebook Model

Status: **implementation baseline**

## 1. Project filesystem model

Default Project storage is WSL-native and owned by the workload user.

Example:

```text
/home/cokernel-user/projects/demo/
├─ pyproject.toml
├─ uv.lock
├─ .venv/                # reconstructible environment state
├─ notebooks/
│  ├─ train.ipynb
│  └─ analysis.ipynb
├─ src/
└─ data/
```

CoKernel does not require this exact subdirectory structure beyond the Project root and dependency files; notebooks may live at arbitrary Project-relative paths.

## 2. uv ownership

uv is authoritative for Python Project dependency/environment operations.

CoKernel operations map to uv concepts:

```text
Create Project  -> initialize Project + sync environment
Add Package     -> uv dependency add
Remove Package  -> uv dependency remove
Sync            -> uv sync / equivalent locked synchronization
Environment Info-> uv/Python inspection
Cache Cleanup   -> uv-supported cache maintenance
```

CoKernel shall not maintain a second dependency database that can diverge from `pyproject.toml`/`uv.lock`.

## 3. Environment storage efficiency

Goals:

- one logically independent environment per Project;
- reuse uv global cache/store/link/clone behavior for identical packages;
- keep Project environment and uv cache on a compatible WSL-native filesystem where possible;
- avoid custom package deduplication logic.

CoKernel may configure a product-scoped uv cache location if this improves ownership/diagnostics, but must use uv-supported mechanisms.

## 4. Environment generation

CoKernel assigns a monotonically increasing `environment_generation` to successful Project environment state changes.

Inputs to environment identity include at minimum:

- relevant dependency declaration state;
- lockfile state;
- selected Python/interpreter state.

A worker records its generation on startup.

If current Project generation differs from Session generation:

```text
Session = STALE_ENVIRONMENT
```

This does not imply automatic process destruction.

## 5. Package mutation while Sessions are active

Policy:

1. Package operation is allowed while Sessions are alive.
2. uv modifies/synchronizes the Project environment.
3. Existing Python processes are not assumed to reflect that mutation.
4. Existing Sessions receive `environment_stale` event.
5. UI/MCP status says restart is required for a guaranteed consistent new environment.
6. User can finish current work before restarting.

This avoids surprise loss of loaded models/variables.

## 6. Project Python selection

The Project Environment Manager owns resolution of the Python interpreter used for new Sessions.

The exact supported Python version range is a release compatibility policy, not encoded into notebook metadata.

On Session start, Runtime resolves one interpreter deterministically and reports it in Session metadata.

## 7. CoKernel worker tooling and user dependencies

CoKernel execution tooling (worker/IPython bridge) is product infrastructure even though it must execute with the Project interpreter.

The chosen launch strategy must satisfy:

- Project package imports work normally;
- CoKernel worker version is controlled by product runtime;
- the user's requested dependency graph remains represented by Project files;
- uv synchronization remains predictable;
- unsupported Python/worker combinations fail before executing a notebook.

A technical spike selects the exact uv-supported launch/injection mechanism before Session implementation is considered stable.

## 8. Notebook format support

v1 targets standard nbformat v4 `.ipynb` JSON.

CoKernel document model shall preserve, as far as possible:

- notebook-level metadata;
- cell IDs;
- cell metadata;
- code/Markdown/raw cells;
- attachments;
- outputs;
- execution counts;
- unknown extension metadata.

Unknown metadata is treated as opaque JSON unless CoKernel explicitly owns a known metadata field.

## 9. Notebook environment metadata

An imported notebook may carry metadata describing the environment used elsewhere.

CoKernel preserves such metadata for compatibility but the active Project determines CoKernel execution environment.

The runtime must not silently launch an unrelated interpreter/environment solely because notebook metadata names one.

## 10. Create Notebook

Create operation:

- target path is Project-relative;
- path traversal outside Project is rejected;
- existing file is not overwritten unless a distinct explicit overwrite operation is introduced;
- a valid empty nbformat v4 notebook is created;
- generated cells receive valid IDs when added;
- document revision begins at 1.

## 11. Import/upload Notebook

Windows import flow:

1. User selects source file through Windows UI.
2. Host reads/streams file; WSL Windows-drive automount is not required.
3. Runtime receives bytes plus Project target path.
4. Runtime verifies Project path containment.
5. Notebook parser validates supported format.
6. Runtime writes a temporary file in target filesystem.
7. Atomic rename installs the notebook.
8. Document is registered/opened.

Policy:

- preserve supported notebook content rather than reconstructing it from cells;
- do not execute code during import;
- import does not automatically install dependencies inferred from source code;
- dependency assistance may be added later as an explicit operation.

## 12. Import conflict behavior

If target path already exists, the initial v1 UX offers explicit choices such as:

```text
Cancel
Choose another name/path
Explicitly replace (only if product exposes a deliberate replace flow)
```

There is no silent overwrite.

## 13. Notebook document authority

While a notebook is open/managed, CoKernel Notebook Document Service is the authority for product-mediated edits and execution-output writes.

Inputs may come from:

- Human UI edits;
- AI/MCP edits if enabled;
- execution outputs;
- metadata operations.

Each accepted mutation increments `revision`.

## 14. Cell execution source

Execution requests refer to:

```text
notebook_id
cell_id
expected/observed document revision
```

Runtime retrieves cell source from the authoritative document state before sending it to worker.

This prevents a caller from claiming to execute Cell X while supplying unrelated code under that identity.

If an explicit scratch-code execution feature is later introduced, it is a separate capability.

## 15. Autosave/output persistence

Initial v1 policy:

- user edits are autosaved after a short debounce and also flushable explicitly;
- execution outputs are persisted after operation completion and may stream to UI before final disk write;
- critical document writes use atomic replace on same filesystem;
- save failure is surfaced and does not falsely mark document clean.

Exact UI debounce timing is configurable implementation detail.

## 16. External file modifications

Runtime monitors known open notebook file identity/hash/mtime sufficiently to detect an external change before overwriting it.

On conflict:

```text
Document = EXTERNAL_CHANGE_DETECTED
```

Normal automatic write is paused until the user chooses a supported reconciliation action (reload, save-as, or explicit replace where available).

Automatic cell-level merge is not required for initial v1.

## 17. Notebook close and Session lifetime

Closing the editor does not inherently mean destroying compute state.

Initial v1 policy:

- closing Notebook view leaves its live Session running;
- Session remains visible in Sessions/dashboard/tray;
- user may stop it explicitly;
- optional idle cleanup policy may be added later but must never silently discard a Session with active/recent work under an undocumented timeout.

This separates UI navigation from compute lifecycle.

## 18. Project delete/remove semantics

The product distinguishes:

- remove Project from CoKernel registry/recent list;
- delete Project files from disk;
- clean Project environment;
- clean shared uv cache.

These are not treated as one implicit destructive action.

Before deleting Project files:

- detect live Sessions;
- warn about Notebook/data loss;
- require explicit confirmation;
- stop affected Sessions deliberately.

Shared uv cache cleanup is separate and delegated to uv-supported cache management.

## 19. Project import/open from Windows filesystem

The core v1 requirement is Project management in WSL-native storage and Notebook import from Windows.

A broader `Import Project Directory` feature can stream/copy a Windows directory into WSL-native Project storage using the same controlled transfer principle. If implemented in v1, it must preserve symlink/path safety and provide progress/cancellation. It is not allowed to re-enable automatic host-drive exposure to worker processes merely for convenience.

## 20. Mandatory tests

- create Project -> valid `pyproject.toml`/lock/environment;
- same package used by multiple Projects follows uv caching without CoKernel duplicate-store logic;
- add/remove/sync updates generation;
- active Session becomes stale rather than killed;
- create Notebook writes valid v4 document;
- import a notebook with Markdown/code/PNG output/unknown metadata and preserve them;
- imported environment metadata does not change Project execution interpreter;
- path traversal rejected;
- existing import target not silently overwritten;
- atomic save survives simulated write failure without corrupting original;
- external modification is detected before overwrite;
- Notebook close leaves live Session visible/running.