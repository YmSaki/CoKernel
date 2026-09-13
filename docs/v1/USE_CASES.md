# CoKernel v1 Use Cases

Status: **implementation baseline**

These use cases translate the requirements into observable workflows. They intentionally describe user/system behavior before implementation mechanics.

## Actors

- **Human User** — operates the Windows application.
- **AI Client** — operates CoKernel through MCP.
- **CoKernel** — product control/runtime system.
- **uv** — Project dependency/environment manager used internally.
- **Execution Worker** — supervised Python/IPython process for one Session.
- **Secure Tunnel** — outbound remote MCP transport.

---

## UC-01 — First installation

**Goal:** make a Windows NVIDIA GPU machine ready for CoKernel use.

**Precondition:** supported Windows host; CoKernel not installed.

**Main flow:**

1. User launches the installer.
2. CoKernel checks Windows/WSL/virtualization/storage/NVIDIA prerequisites.
3. CoKernel creates the dedicated managed Linux environment.
4. CoKernel installs its runtime and required Linux tooling.
5. CoKernel verifies the GPU is visible from the managed runtime.
6. CoKernel starts the background Host/runtime.
7. Desktop reports the machine ready for Project creation.

**Alternate/error flows:**

- reboot is required to finish Windows/WSL enablement;
- GPU driver unsupported or GPU unavailable;
- insufficient disk space;
- legacy experimental CoKernel detected -> explicit Legacy Reset flow;
- network unavailable for required downloads.

**Success condition:** Project creation is available and runtime health is healthy.

---

## UC-02 — Create Project

**Goal:** create a new isolated Python workspace.

**Main flow:**

1. User selects `New Project`.
2. User provides name and supported Project options.
3. CoKernel creates Project storage.
4. CoKernel initializes the uv project/dependency definition.
5. CoKernel synchronizes the Project environment.
6. Project appears ready for package and notebook operations.

**Success condition:** Project has valid dependency state and runnable environment.

---

## UC-03 — Open existing Project

**Goal:** use an existing compatible Project.

**Main flow:**

1. User selects an existing Project known to CoKernel or imports/registers one through a supported path.
2. CoKernel reads its dependency definition.
3. CoKernel checks environment status.
4. If required, CoKernel reconstructs/synchronizes the environment with uv.
5. Files/notebooks become available.

**Success condition:** Project can start a Session without manual virtualenv selection.

---

## UC-04 — Add package

**Goal:** add a dependency to the current Project.

**Main flow:**

1. User chooses `Add Package` and package/version constraints.
2. CoKernel delegates dependency resolution/install to uv.
3. `pyproject.toml`/`uv.lock` are updated on success.
4. Environment is synchronized.
5. Package list reflects the new state.
6. Existing live Sessions are marked as using a pre-change environment snapshot and shown as requiring restart to reliably use the new package state.

**Error flows:** resolver conflict, package not found, download error, disk full, incompatible platform.

---

## UC-05 — Remove/synchronize packages

**Goal:** change or restore Project environment state.

**Main flow:**

1. User removes a package or requests `Sync`.
2. CoKernel delegates to uv.
3. Dependency/environment state is updated.
4. Live Sessions remain alive but are marked environment-stale where applicable.
5. User may restart Sessions when ready.

**Important rule:** dependency operations do not silently destroy live in-memory work.

---

## UC-06 — Create Notebook

**Goal:** create a new notebook document in a Project.

**Main flow:**

1. User selects `New Notebook`.
2. User chooses a Project-relative path/name.
3. CoKernel creates a valid standard `.ipynb` document.
4. Notebook opens for editing.
5. No Session is required merely to edit/save the document.

---

## UC-07 — Import/upload Notebook

**Goal:** bring an existing `.ipynb` into a Project.

**Examples:** notebook from Downloads, GitHub, VS Code, another PC, another person, or an AI.

**Main flow:**

1. User selects `Import Notebook` in a Project.
2. Windows file picker selects an `.ipynb` file.
3. CoKernel validates that the document is structurally readable.
4. CoKernel transfers/copies it into the Project through the managed import path.
5. Supported cells, outputs, attachments, execution counts, and metadata are preserved.
6. Notebook opens as a normal Project notebook.
7. When executed, it uses the target Project environment regardless of unrelated environment hints carried in notebook metadata.

**Alternate/error flows:** malformed notebook, unsupported major format, target path collision, oversized/failed transfer, insufficient Project storage.

**Success condition:** imported file remains a normal valid `.ipynb` and is executable in the Project.

---

## UC-08 — Open Notebook and execute a cell

**Goal:** interactively execute notebook code using the Project environment.

**Main flow:**

1. User opens `experiment.ipynb`.
2. User selects a code cell and runs it.
3. If no primary Session exists, CoKernel starts one using the Project environment.
4. CoKernel queues the execution request to that Session.
5. Worker executes with interactive/IPython semantics.
6. CoKernel receives stdout/stderr/result/display/error events.
7. UI renders the result.
8. Notebook output is persisted atomically.
9. Session remains alive with its namespace for subsequent cells.

**Success condition:** another cell can use variables/imports created by the first cell.

---

## UC-09 — Close and reopen Notebook

**Goal:** leave the editor without confusing document persistence and live memory state.

**Main flow:**

1. User closes the notebook view.
2. Saved `.ipynb` remains durable.
3. If product policy keeps the Session alive, UI/tray continues to show that Session separately from the closed document view.
4. Reopening the notebook can reconnect to the existing primary Session while it remains alive.

**After runtime/Windows restart:** notebook remains; volatile Session memory does not automatically reappear.

---

## UC-10 — Run multiple notebooks in the same Project

**Goal:** parallel independent notebook work using one Project environment.

**Main flow:**

1. `train.ipynb` starts Session A.
2. `analysis.ipynb` starts Session B.
3. Both use the same Project dependency environment.
4. A and B have separate Python processes/namespaces.
5. A and B may execute concurrently subject to machine resources.

**Success condition:** one Session's variables do not appear in the other merely because they share a Project.

---

## UC-11 — Run notebooks from different Projects concurrently

**Goal:** concurrently run different dependency environments.

**Main flow:**

1. Project A Session starts with Project A environment.
2. Project B Session starts with Project B environment.
3. Both execute concurrently.
4. Resource dashboard reflects machine and Session states.

**Failure consideration:** CPU/RAM/GPU/VRAM are finite; concurrency never implies guaranteed resource availability.

---

## UC-12 — Interrupt current execution

**Goal:** stop a long/infinite current cell without intentionally losing the whole Session.

**Main flow:**

1. User/AI requests `Interrupt`.
2. CoKernel targets the currently running execution in the selected Session.
3. Worker attempts cooperative/interpreter-level interrupt.
4. Session returns to idle if it survives.
5. Result is recorded as interrupted.

**Escalation:** if the worker does not respond, user may terminate/restart the Session explicitly.

---

## UC-13 — Restart Session intentionally

**Goal:** return a notebook to a clean Python memory state.

1. User selects `Restart Session`.
2. CoKernel warns that in-memory variables/objects will be lost.
3. Old worker is stopped.
4. New worker starts with current Project environment.
5. Notebook document/output files remain unless separately cleared.

---

## UC-14 — Session crashes

**Goal:** understand and recover from unexpected Python/native failure.

**Trigger examples:** process abort, SIGSEGV, SIGKILL, OOM, CUDA/native library crash, runtime failure.

**Main flow:**

1. Supervisor detects worker exit/disconnect.
2. Session enters `CRASHED`.
3. CoKernel freezes failure evidence before replacement/restart.
4. It correlates available process exit status/signal, worker stderr, last operation/cell, Linux OOM evidence, memory and GPU state, and runtime events.
5. UI states what is known and what remains unknown.
6. User chooses restart, inspect details, or leave stopped.

**Success condition:** failure is not reduced to an unexplained restart loop.

---

## UC-15 — Inspect resources

**Goal:** know whether current compute activity is healthy and what consumes resources.

**Main flow:**

1. User opens Resources/Dashboard.
2. CoKernel shows host CPU/RAM/storage, WSL RAM, GPU/VRAM, runtime status, active Sessions.
3. Where reliable, per-Session resource attribution is shown.

---

## UC-16 — AI joins human's live Notebook Session

**Goal:** human and AI share the exact same in-memory compute state.

**Main flow:**

1. Human opens notebook and starts its Session.
2. Human executes cells creating variables/models.
3. AI lists/identifies Project and notebook through MCP.
4. AI requests the notebook's current Session.
5. CoKernel attaches AI operations to the existing primary Session.
6. AI can inspect supported variables or execute selected notebook cells against that same namespace.

**Invariant:** CoKernel does not silently create a second Session merely because AI joins.

---

## UC-17 — AI reads a Notebook without running it

**Goal:** inspect document content with no execution side effect.

1. AI requests notebook read operation.
2. CoKernel returns notebook structure/content under bounded response rules.
3. No Session is started solely for document reading.

---

## UC-18 — AI safely inspects a variable

**Goal:** read simple live state without giving arbitrary expression/code input.

1. AI selects an existing Session.
2. AI requests `get_variable` with one valid identifier.
3. CoKernel returns bounded safe values for explicitly supported exact built-in types.
4. Unsupported/custom object values return type/unsupported metadata without invoking arbitrary `repr`, attribute access, or custom iteration.

---

## UC-19 — Human and AI request execution concurrently

**Goal:** avoid undefined interleaving in one shared namespace.

1. Human queues Cell A.
2. AI queues Cell B while A is running.
3. Both requests enter the Session's single FIFO execution queue.
4. A completes/fails/interrupts.
5. B executes next.
6. Both clients receive operation IDs and state updates.

**Invariant:** one Session executes one cell/code operation at a time. Different Sessions may execute concurrently.

---

## UC-20 — Enable remote AI access

**Goal:** use MCP from an external ChatGPT client without public inbound networking.

1. User enables Remote AI Access.
2. User supplies/authorizes required tunnel credentials.
3. CoKernel stores external credentials in the Windows secret store.
4. Local MCP service starts/authenticates.
5. Managed tunnel establishes outbound connection and routes to the local MCP service.
6. UI reports connected/ready state.

---

## UC-21 — Tunnel/network disconnects

**Goal:** keep local compute usable during remote-access failure.

1. Tunnel loses network/control-plane connection.
2. Remote AI becomes unavailable.
3. Local Projects, Sessions, package operations, and notebook execution continue.
4. CoKernel reports remote access degraded and retries with bounded backoff.

---

## UC-22 — Windows/runtime restart

**Goal:** return the product to an understandable state after machine restart.

1. Windows restarts.
2. CoKernel Host starts according to user setting/intended state.
3. Managed WSL/runtime becomes available.
4. Projects and notebook files remain.
5. Previous in-memory Sessions are shown as ended/lost rather than implied to have survived.
6. User can start fresh Sessions.

---

## UC-23 — Product update with active Sessions

**Goal:** update without surprising data/state loss.

1. Update is available.
2. CoKernel determines whether applying it would interrupt live Sessions.
3. If not disruptive, update may proceed according to policy.
4. If disruptive, user is told which Sessions are affected and can defer or explicitly proceed.
5. Post-update health acceptance runs.

---

## UC-24 — Delete Project / clean storage

**Goal:** remove work without corrupting shared uv cache semantics.

1. User requests Project deletion/removal.
2. CoKernel checks live Sessions and unsaved/import activity.
3. Destructive user-owned file removal requires clear confirmation.
4. Project removal does not blindly delete global/shared uv cache entries still useful elsewhere.
5. Cache cleanup is a separate maintenance operation.

---

## UC-25 — Diagnose product/runtime failure

**Goal:** distinguish notebook failure from infrastructure failure.

CoKernel shall identify the affected layer where possible:

```text
Machine / Windows
Managed WSL runtime
Project environment
Execution Session
MCP service
Remote tunnel
```

The user can export a redacted diagnostic bundle and is given an actionable repair/retry path when known.

---

## Representative v1 acceptance journeys

The following seven journeys are mandatory release narratives:

1. **Install:** Windows PC -> CoKernel -> Linux GPU compute ready.
2. **Project:** create Project -> add package -> environment ready.
3. **Notebook:** create/import `.ipynb` -> run cells -> persistent state.
4. **Parallel:** multiple notebooks -> independent concurrent Sessions.
5. **Failure:** Session crashes -> useful failure evidence -> intentional recovery.
6. **Human + AI:** human-created in-memory value -> AI sees it through same Session.
7. **Remote AI:** ChatGPT -> secure tunnel -> MCP -> same local Session.