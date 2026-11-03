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

Project creation/registration uses the asynchronous setup contract in
INTERFACE_CONTRACTS section 21. An incomplete creation retains its identity and
files with `initialization_required`; synchronization resumes uv initialization
before syncing, without replacing an existing pyproject or deleting user files.

Package add/remove/sync admission and completion follow INTERFACE_CONTRACTS
section 20. Runtime persists an uncertainty marker before uv starts, reconciles
failed attempts as well as successful ones, and refuses new Session startup until
an uncertain environment is synchronized. Existing Session memory is preserved.
An uncertain attempt can conservatively advance environment_generation even when
dependency-file fingerprints match; the generation also invalidates uncertain
installed environment state. A no-op successful sync need not advance it.

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

## 21. Initial document implementation contract

The first document slice lives in Runtime's `notebook` module. Its JSON model is
platform-independent; authoritative persistence is Linux-only, matching the WSL
Runtime boundary. Desktop does not write Project notebook files directly.

### Format and limits

- Read nbformat 4.0 through 4.5. Unsupported versions, cell/output types and
  malformed required fields fail explicitly; they are never silently dropped.
- Preserve unknown JSON fields and opaque metadata, attachments, rich MIME,
  stream/error outputs, source string-versus-string-array representation and
  existing valid unique IDs. Duplicate JSON object keys are rejected.
- Legacy 4.0–4.4 documents receive IDs for cells missing them and normalize to
  4.5 in memory. This makes the document dirty; opening does not rewrite disk.
  Invalid/duplicate IDs and missing IDs in a declared 4.5 document are errors.
- Bounds: 64 MiB serialized document, 10,000 cells, 1,000 edits per transaction,
  128 loaded documents and 256 MiB of their serialized content per Runtime.
  These are admission bounds, not a claim about total allocator/RSS usage.
- Project discovery examines at most 100,000 entries, 32 directory levels and
  10,000 notebook paths. It skips symlinks and `.venv`, `.git`, `.cokernel`
  directories, returns sorted Project-relative paths, and does not parse files
  merely to list them. Malformed notebook files are reported when opened.

The supported structural shapes follow the
[nbformat description](https://nbformat.readthedocs.io/en/latest/format_description.html).
The parser validates owned structural fields; it does not interpret arbitrary
third-party metadata or install/launch an imported kernelspec.

### Identity, revisions and state

One Runtime service owns documents, deduplicated by Project ID and relative path.
Notebook IDs and revisions are Runtime-lifetime identities; clients refresh their
references after Runtime replacement. Opening an already managed path retains
its identity and unsaved edits. Runtime restart loses volatile Session state;
saved `.ipynb` content remains the durable document truth.

An edit batch validates a cloned candidate and commits in memory as one revision.
`set_cell_source`, `insert_cell`, `delete_cell`, `move_cell`,
`set_notebook_metadata` and `set_cell_metadata` require an expected revision.
Metadata-setting operations replace the specified metadata object deliberately;
source/reorder operations preserve unrelated metadata. Move index means final
index after movement. Inserts without an ID receive a generated one. Invalid
batches leave the document and its revision unchanged. Stale revisions return
`CK-NB-CONFLICT` with the current revision in the typed error.

State is `CLEAN`, `DIRTY`, `EXTERNAL_CHANGE_DETECTED` or `SAVE_FAILED`.
`content_hash` hashes current compact JSON plus newline;
`saved_content_hash` hashes actual last-observed persisted bytes. Saving unchanged
content does not create an edit revision. Explicit reload replaces in-memory
content and increments revision; a caller must explain that unsaved edits are
discarded. Neither operation mutates Session memory.

### Filesystem and save behavior

The service obtains its Project from the Project manager, never from an arbitrary
client root path. It opens the Project directory, walks relative directories
through descriptor-relative `openat` with `O_NOFOLLOW`, rejects traversal,
absolute/Windows-style paths, symlink components, non-regular files and multiply
linked notebook files. Parents must already exist. Create uses atomic
`renameat2(RENAME_NOREPLACE)`; collision never overwrites a file.

Save writes a private same-directory temporary file, syncs it, checks the original
file's device/inode/size/timestamps/SHA-256 again, then atomically replaces and
syncs the directory. Failure before replacement removes temporary data and leaves
the original intact. Failed directory sync is reported as `CK-NB-SAVE` with
`SAVE_FAILED`, even if replacement is already visible; it never reports false
durability. Unsaved content stays available for recovery.

Detection is optimistic against external tools: Linux filesystems do not provide
a hash-conditional atomic rename. An uncooperative writer can still modify a file
in the final check-to-rename interval. Product-mediated mutations are serialized;
this mechanism is not isolation from a hostile process with write access to the
same Project directories. Existing conflicts stop normal writes until explicit
reload or a later save-as reconciliation path.

The Linux Runtime bridge exposes these document operations through the shared
domain coordinator. MCP, Host event forwarding and Desktop autosave orchestration
are later-phase adapters; they must use this same document authority.

## 22. Bounded import and output application

Runtime import methods are `begin_import`, `import_chunk`, `finish_import` and
`cancel_import`. A random typed ImportId binds the transaction to Project ID,
Project root and target path. Size must be 1..64 MiB and the declared SHA-256
must contain exactly 64 hexadecimal characters. The target must not exist.

- Admission permits four active imports and 128 MiB total declared staging size.
- Chunks contain 1..262,144 decoded bytes and exactly the next expected offset.
- The RPC adapter bounds base64 text to 349,528 bytes before decoding and invokes
  this byte-oriented domain API. Malformed or oversized base64 aborts the owned
  transaction; wrong-Project requests cannot abort it.
- Staging is an anonymous `O_TMPFILE` in the target filesystem. Closing the
  transaction or Runtime process removes it without named partial files.
- A transaction expires five minutes after begin, not after its latest chunk.
  Expired transactions are rejected and collected on the next import API call;
  idle staging is always bounded and is released on service shutdown.
- Wrong-Project calls do not cancel another Project's transaction. Invalid chunks,
  incomplete finish, hash mismatch, malformed document or finish failure discard
  that transaction; continuation requires a new begin.
- Finish checks total bytes and SHA-256, rereads and rechecks staged content,
  validates the notebook, re-resolves the target safely and installs through the
  document service's no-overwrite atomic write. Parent swaps/symlinks and files
  created during upload are rejected. Import does not execute or install packages.
- An already-managed document retains its identity even if its file disappeared
  externally. Create/import cannot establish a second identity over that path.

`NotebookOutputs` converts complete captured worker results into nbformat stream,
display_data, execute_result and error outputs. It rejects missing/gapped events,
mixed Session/operation IDs, invalid output shapes, inconsistent final accounting
and missing terminal responses. `apply_outputs` uses the same expected-revision,
Project ownership and external-change checks as edits, then marks the document
dirty for ordinary save. Only the target cell outputs/count and the owned
`metadata.cokernel.execution` field change; unrelated metadata and cells remain.
Execution metadata records operation/status and output truncation accounting.

## 23. Authoritative execution and view lifetime

`NotebookRuntime` coordinates the document service and existing Session
Supervisor. Admission validates Project/Notebook/Session ownership, the expected
document revision and a code cell ID. Source is resolved internally; public
execute requests cannot supply source or caller origin. The transport adapter
supplies the authenticated Human/MCP origin separately.

Each accepted execution captures a private source-generation token and monotonic
execution ordinal. Output revisions and edits to other cells preserve that token.
Editing the target source (even to the same text), deleting/reinserting its ID,
or explicitly reloading the document invalidates it. A late completion must not
replace a newer completion for the same cell. These cases return
`CK-NB-STALE-EXECUTION`; accepted code may still have changed Session memory.

A Runtime-owned task receives the complete captured result, applies outputs to
the current document revision under the document lock and saves atomically.
It survives the requesting caller's cancellation after acceptance. At most 32
document executions are pending globally; 128 compact operation records are
retained in Runtime memory, evicting old completed records first. Clients poll
`operations.get` to distinguish execution status from persistence status.
`NOT_PERSISTED` explicitly reports capture/conflict/save failures; it never claims
that a successful Python execution implies a saved notebook. Unsaved output that
has already been applied remains in the document for save retry.

Closing a view flushes the document at the supplied revision and keeps its
NotebookId and primary Session association. Flush failure is returned, never
silently discarded. Explicit reload discards unsaved edits without resetting
Python memory. Non-execution edits stay DIRTY until save/close; Desktop's short
debounce autosave is implemented with the UI in Phase 8. Save-as reconciliation
remains a future extension. Runtime process shutdown still loses volatile Session
memory and retained operation status; Host/UI lifetime separation is Phase 5.

Document work now has an owned FIFO queue. Public file operations, execution
preparation, output commit/save and shutdown flush run through that queue's
blocking-pool jobs. Domain admission and worker control do not hold its filesystem
waits. Queue cancellation, bounds and shutdown refusal are specified in
INTERFACE_CONTRACTS section 24. Source/revision and atomic-write guarantees above
remain authoritative; moving the work does not permit stale results to overwrite
newer edits or give workers a persistence path.

Notebook Service also maintains a bounded loaded-identity index alongside its
document table. Registration updates it before returning a new Notebook ID.
The index contains only Notebook/Project IDs, allowing lifecycle admission to
reject cross-Project access without acquiring the document filesystem lock.
It is not a cache of document contents or disk validity; owned document work still
performs authoritative path, revision and external-change checks.

Project registry storage likewise has a single owner and read-only projection.
Prepare, write-ahead uncertainty, status probes, canonical registration lookup,
post-uv fingerprint/reconciliation and registry removal run as owned jobs. uv
execution remains parallel across different Projects, with each Project reserved
through reconciliation. Registration aliases resolve to the same reservation.
Projection updates follow the storage attempt, including conservative uncertainty
on failure. Registry removal failure restores the in-memory record for retry and
never deletes Project files. See INTERFACE_CONTRACTS section 26 for admission,
capacity, failure and shutdown behavior.
