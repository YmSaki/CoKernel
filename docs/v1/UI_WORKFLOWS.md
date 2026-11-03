# Desktop workflow and information placement — 2026-09-24

Status: **external design reopened; concrete proposal for review**.
This refines UI_SPEC sections 2–10; it does not change Project/Notebook/Session
ownership. The initial API-summary Desktop screen was rejected by the user.
Its successful build/connection is transport evidence, not evidence of usability.

## Why the first screen failed

- The central surface presented infrastructure status rather than the user's work.
- Project creation remained visible inside an unrelated existing Project.
- Internal paths, IDs and generations displaced notebook actions.
- Merely listing notebook names offered no path into editing or execution.
- The seven-page navigation catalog did not specify which information must stay
  visible during editing or how the user reaches relevant recovery actions.

Do not ask the user to choose abstract layouts before showing concrete contents
and flows. The preview uses labeled sample data and sends no Host/MCP requests.

## Information placement

| Place | Always visible | On demand | User purpose |
|---|---|---|---|
| Project selector, upper left | Current Project name | Known Projects; create/register actions; path/details | Choose the dependency boundary without choosing a Python executable |
| Left notebook list | Notebook paths and running/waiting/error badges | Create and Windows import actions | Find the document and notice another notebook still running |
| Center document tabs | Open document names, unsaved marker | Close view; reopen from left list | Keep document navigation separate from Session lifetime |
| Notebook header | Name; document save state; current execution state | Session details and recovery controls | Know independently whether code is saved and whether Python is busy |
| Cell | Source, execution count, run action, output immediately below | Add above/below, move, delete; output details | Edit/run/read without leaving the notebook |
| Right inspector (closed until requested) | Selected inspector only | Variables; current/queued operations with Human/AI origin; failure evidence | Understand the existing live namespace and waiting execution |
| Project environment view | Dependency list, environment readiness, active package operation | Add/remove/sync, resolver details | Manage this Project's environment; keep notebook drafts intact while visiting |
| Bottom status strip | Local compute availability; resource summary; remote AI status | Machine resources; all running notebooks; remote settings; diagnostics | Notice health without losing the notebook workspace |
| Project selection screen | Existing Projects, create/register, actionable setup failure | Advanced paths and diagnostic details | Start work when no Project is selected |

UUIDs, PIDs, environment generations, Runtime protocol and Linux executable paths
belong in copyable Details, not cards in the normal work surface. A connected
tunnel means remote access is available; it does not prove a particular AI client
is currently attached. Only attributed operations may be labeled Human/AI.

## Concrete journeys

### Create or enter work (UC-02, 03, 06, 07)

1. With no selected Project, show the Project selection screen.
2. New Project opens a name form. Show preparation progress and cancel if admitted;
   enable notebook actions when the authoritative environment state permits them.
3. Selecting a Project opens its notebook list. Empty center explains how to
   create or import a notebook; it does not display another Project creation form.
4. New Notebook asks for its Project-relative name. Import uses a Windows picker,
   displays the destination and requires an explicit alternative on collision.
5. Create/open returns a document tab. Merely viewing it starts no Python Session.

### Edit, run and revisit (UC-08, 09)

1. Source editing marks the document unsaved. Source edits use the current revision;
   save completion is required before a run is submitted for that source.
2. A cell run implicitly ensures the notebook's primary Session, then queues the
   stored cell. The user does not select a kernel or supply code a second time.
3. The cell shows queued/running/completed state; output appears below its source.
   A Python exception is a cell error, not automatically a Session crash.
4. Closing a document tab leaves its Session running. Pending unsaved edits must
   be saved or explicitly discarded before that view can be removed.
5. Reopen reconnects to the same primary Session when it still exists.

### AI and other notebooks (UC-10, 11, 16–19)

1. While AI is running the current notebook, the header says so and the inspector
   identifies the current cell and queued work. The human can continue editing.
2. Human Run queues behind accepted work. Interrupt clearly targets the current
   operation, including an AI-origin operation; it is not labeled "cancel mine".
3. Switching to another notebook changes document and primary Session together.
   The left list keeps the earlier notebook's execution badge visible.
4. The global running-notebooks view groups by Project and names notebooks;
   selecting a row opens that notebook. No internal Session picker is introduced.

### Package change (UC-04, 05, 13)

1. Environment opens the current Project's package management view.
2. Add/remove/sync shows progress, cancellation and failure at that operation.
3. Success does not restart any Session. Affected notebooks show "old environment
   still in use" and retain their variables.
4. Restart is explicit and names the affected notebook and lost volatile memory.
   Returning to the notebook preserves its editing state.

### Recovery and uncertainty (UC-12–15, 21, 22, 25)

| Condition | Placement and behavior | Actions |
|---|---|---|
| Revision conflict / external file change | Notebook banner; preserve local text and remote version; block blind save/run | Compare/reload with explicit draft-discard choice; copy draft |
| Python exception | Error output under the cell; keep Session state distinct | Edit/rerun; inspect traceback |
| Session crash | Notebook banner and left-list badge; show known evidence and unknown cause explicitly | View evidence; restart after memory-loss explanation; leave stopped |
| Environment stale | Notebook header warning and environment view | Keep running; explicit restart |
| Remote AI disconnected | Remote status strip/drawer only; local run remains available | Inspect/reconnect remote access |
| Host connection lost | Workspace banner; retain drafts and mark displayed state stale | Automatic observation reconnect; refresh authoritative state; never replay mutations |
| Mutation outcome unknown | Message beside the initiating operation | Inspect authoritative operation/document state before an explicit retry |
| Runtime stopped/restarted | Workspace status plus affected Session state | Start Runtime or a new Session explicitly; never imply old variables survived |
| Metrics unavailable/stale | Value replaced by reason/age | Refresh or inspect diagnostics; never display missing data as zero |

## Review and implementation sequence

The [interactive preview](ui-preview/index.html) demonstrates placement, notebook
switching, packages, variables/queue, resources, remote access and error states.
All content is sample data. Simulated controls cannot be used as acceptance proof.

1. Settle this content/flow model with concrete feedback on the preview.
2. Replace the rejected summary screen with the workspace shell using existing
   Host IPC. Separate presentation state from document drafts and remote state.
3. Wire create/import/open/edit/save/run; prove real Desktop-origin state sharing.
4. Wire package operations, Session controls, queue/variables and failure recovery.
5. Complete resources, Remote AI, diagnostics and lifecycle acceptance.

No split-editor behavior is assumed necessary at this point. The preview keeps
one visible document and demonstrates switching while computation continues;
this is a reviewable proposal, not an inferred user preference.

## Impact and acceptance

Primary owner: upstream Phase 5 Basic/External Design, REOPENED → ACTIVE.
Still valid: product purpose, user-confirmed per-notebook primary Session, uv
Project boundary, Tauri/Host/Runtime dependency direction, P0 runtime proofs.
Affected: UI_SPEC navigation/placement, P8-03..14 UI implementation and P8-A-01.
P8-01 build/launch and transport tests remain narrow technical evidence.

Acceptance must demonstrate: opening/running an imported notebook without routine
CLI; finding the current Project environment; seeing the correct live namespace;
recognizing Human/AI queue order; switching notebooks without losing state;
distinguishing save conflict, Python error, Session crash and remote disconnect;
and closing the UI without stopping compute. These are observable tasks, not
pixel- or component-count tests.
