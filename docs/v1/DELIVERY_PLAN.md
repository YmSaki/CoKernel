# Desktop / Codex / ChatGPT shared-notebook delivery plan

Status: active — 2026-09-21
Starting revision: `f35ca99711a8d6c102ef9e47b857e0e377f87cbb`

## Objective and completion boundary

The user operates notebooks through a Windows Desktop UI. Codex and web ChatGPT
connect through CoKernel-native MCP and use the same notebook primary Session.
An actual three-client shared-state demonstration is required; simulated callers
and worker-only tests cannot close this delivery goal.

User clarification (2026-09-21): Linux/WSL is used to enable JAX. The delivery
therefore includes a uv Project with working JAX on this workstation's NVIDIA
GPU, executed through a CoKernel Session. Import success alone is insufficient:
record GPU device discovery and a completed representative JAX computation,
then demonstrate access to that Session from the human and AI client paths.

The accepted `docs/v1` requirements, architecture and implementation order remain
authoritative. `WORK_STATUS.md` remains the phase/item ledger. This plan adds the
delivery sequence and review-discovered repairs; it does not waive phase gates.
Human browser UI, multiple compute machines and Jupyter service integration are
outside this delivery. Desktop UI is required.

## Delivery packages

| Package | Existing phase / dependencies | Work | Required evidence |
|---|---|---|---|
| D01 Verification | Phase 3, first | Make Windows checks fail on every failed native command; provision reproducible local Linux/WSL developer dependencies; resolve actual fmt/build/lint/test failures | Failure-injection regression, Windows checks, Linux worker + Rust checks |
| D02 Durable failures | Phase 3, D01 tooling | Store bounded FailureRecords before announcing crash; expose later retrieval; preserve evidence across Session restart; surface storage failure | No-subscriber crash, reopen store, restart, corrupt/oversized input, storage failure tests |
| D03 Process ownership | Phase 3, D01 tooling | Own worker process group; reap descendants on stop/restart/crash/startup failure; keep other Sessions alive | Worker with a real child process, forced termination, other-Session survival |
| D04 Engine gate | Phase 3, D01–D03 | Run and repair real Supervisor/worker tests; verify persistent state, FIFO, parallel Sessions, interrupt, stale environment and hostile inspection | Canonical check.sh/check.ps1 and real-process smokes; recorded versions and results; P3-GATE |
| D05 Notebook documents | Phase 4, D04 | nbformat v4 preservation, create/open/import, revisioned edits, atomic save, external conflict detection, authoritative cell execution and output persistence | Compatibility fixtures, stale revision, interrupted save, path/symlink/collision tests, Project import/execute/save proof |
| D06 Runtime and Host | Phase 5, D05 | Join Project/Notebook/Session services; connect environment mutations to stale notification; async operations/events; persistent WSL bridge and current-user Named Pipe | Human-style client survives reconnect; independent concurrent Sessions; package mutation preserves memory; Windows import |
| D07 Local MCP | Phase 6, D06 | Authenticated Streamable HTTP, shared Runtime API client, required tools and bounds, honest annotations; configure actual Codex connection | Actual Codex reads a value created through the human path in the same Session, plus negative auth/ID/revision tests |
| D08 Remote MCP | Phase 7, D07 | Credential isolation, managed outbound tunnel, readiness/reconnect, ChatGPT-compatible authenticated connection | Actual web ChatGPT shared-state proof; tunnel disconnect does not stop execution; no product secret in worker or diagnostics |
| D09 Desktop | Phase 8, D08 | Tauri UI for Projects/packages/notebooks, cell editor/run/output, Session controls, stale/save/conflict/failure status, resources and remote settings | Real human workflow without routine CLI, UI close/reopen preserves Session, three-client shared-state proof |
| D10 Installation and maintenance | Phases 9–10, D09 | Dedicated managed WSL provisioning/hardening, first-run checks, installer, preserve-data update/repair and active-Session guards | Non-destructive install/update/repair evidence; explicit authorization for any legacy data deletion |
| D11 Final acceptance | Phase 11, D10 | Execute relevant L0–L6 gates and all canonical proofs with actual clients | JAX GPU computation in the Project Session, actual Desktop/Codex/ChatGPT shared Session, crash and disconnect survival, valid saved ipynb |

Packages follow the existing numeric phase gates. An unfinished lower gate is
fixed before claiming dependent package completion. Later package tasks are
already decomposed in WORK_STATUS; refine their internal contracts when their
predecessor gate is satisfied rather than inventing a second independent WBS.

## First implementation batch

| Task | Change boundary | Acceptance | State |
|---|---|---|---|
| D01.1 | scripts/v1/check.ps1 and behavior tests | Failed Python/cargo/uv command stops before subsequent stages and PASS, even with default PowerShell native-error settings | DONE — 19 injected stages + success |
| D01.2 | Private developer tooling, dependency locks, formatting | Linux build and real worker tooling can run reproducibly; no system Python replacement or changes to unrelated WSL distributions | DONE — native Linux source snapshot, locks, Windows + Linux build/lint |
| D02.1 | Runtime failure store + Session lifecycle + spec refinement | Crash record readable without a live subscriber and after reopening; bounded records/retention; storage failure explicit | DONE — unit/process evidence |
| D03.1 | Worker spawn/termination + lifecycle tests + spec refinement | Stopped/crashed/restarted Session leaves no ordinary descendants; another Session remains usable | DONE — real descendants and survivor tests |
| D04.1 | Existing canonical tests, evidence and WORK_STATUS | Rust/worker/WSL tests actually pass and Phase 3 acceptance is recorded | DONE — Linux and combined Windows/WSL gates PASS; source identity verified |

Evidence: [2026-09-21 local verification](evidence/phase3-local-20260921.md).
Next package: D05 / Phase 4 notebook documents and execution integration;
component progress and the next actionable work are recorded below.
The overall goal remains active; Desktop/MCP/JAX GPU acceptance is still required.

## D05 document component batch — 2026-09-21

Model/metadata preservation, create/open/list, revisioned edits, atomic save and
external-change detection are implemented and component-verified. Linux's full
Rust workspace passes 115 tests (18 new notebook tests); Windows passes 36 tests.
The compatibility fixture also passes nbformat 5.11.1 validation. See
[document evidence](evidence/phase4-documents-20260921.md).

Output conversion/application, reliable captured completion and bounded import
transactions are now component-verified; see
[import/output evidence](evidence/phase4-import-output-20260921.md). Linux passes
128 Rust tests and the extended real IPython Supervisor smoke; Windows passes 39.
Authoritative execute-cell admission, queued-source conflict handling,
Runtime-owned persistence and close/flush are now verified through the actual
framed Runtime, uv Project and real worker. D05 / P4-GATE pass locally; see
[Runtime acceptance](evidence/phase4-runtime-20260921.md). The combined Windows/WSL
canonical gate passes. Source/origin injection, source ABA/reload conflicts,
late completions and large document transport have explicit regression evidence.

Next is D06 / Phase 5: persistent WSL bridge ownership, current-user Named Pipe,
event correlation, reconnect/backoff, metrics and Windows file streaming.
Document save-as remains an extension; Desktop debounce autosave belongs to D09.
The overall goal stays active until actual Desktop/Codex/ChatGPT sharing and JAX
GPU computation are demonstrated.

## D06 Host supervision batch — 2026-09-21

P5-01..P5-05 pass locally: independent Windows Host binary, owned persistent WSL
bridge, bounded protocol handshake, persisted desired state and bounded recovery.
The real Windows Host -> wsl.exe -> Runtime -> uv/IPython proof demonstrates
shared Session state after a caller is dropped, dirty-document flush on stop,
worker exit and reopening saved data after restart. Failed explicit shutdown
retains the document connection for reconciliation; STOPPED is never silently
reinterpreted as a reconnect request. See
[Host component evidence](evidence/phase5-host-core-20260921.md).

D06 remains open. P5-06 and P5-10 now pass the current-user Named Pipe and Windows
file streaming boundary: explicit ACL/peer identity, bounded multiple clients,
accepted work across disconnect, local file handle checks and actual chunked import
into a WSL Project. Saved notebook validation and worker exit pass independently.
See [pipe/import evidence](evidence/phase5-pipe-import-20260921.md).

P5-07 now forwards Runtime events and initial/reconnect domain snapshots. A second
real Windows Named Pipe client observes execution start, output and persisted
completion without polling; bounded event queues report loss explicitly and carry
Runtime generation/sequence for reconciliation. Inspection responses remain private.
See [event and interrupt evidence](evidence/phase5-events-20260921.md).

The event integration exposed an early-interrupt race that could terminate the
worker. A deterministic failing test preceded the repair: Supervisor waits for
the worker's guarded start acknowledgement before signalling, and the worker
handles start/cleanup interrupts without corrupting protocol or capture state.
The complete Windows/WSL gate and actual pipe proof now pass.

P5-07 now admits package add/remove/sync as bounded asynchronous operations and
keeps Session interrupt/status available while uv runs. It publishes completion
without polling, supports cancellation, and cancels/reaps uv on bridge EOF.
Write-ahead environment uncertainty survives Runtime restart and prevents new
Session startup until successful synchronization. Live Session memory is retained.
The full Windows/WSL gate and actual Host package-event proof pass; a reproduced
registry-commit failure also has a verified repair. See
[async package evidence](evidence/phase5-async-packages-20260921.md).

Project create/register now also return asynchronous operations. Cancellation
retains files/identity, sync resumes incomplete initialization, and EOF/Runtime
restart preserves the repair state. The full Windows/WSL gate and actual Host
creation-completion proof pass; see
[Project setup evidence](evidence/phase5-project-setup-20260922.md).

P5-07's functional proof passes: long Session startup/inspection replies now wait outside bridge
admission with bounded capacity, correlated replies, primary coalescing and
Project mutation exclusion. Different notebooks start concurrently; stop/EOF
cleanup and responsive control pass actual-worker and Windows Host proofs plus
the full gate. See [Session RPC evidence](evidence/phase5-session-rpc-20260922.md).
The user's architecture review reopened P5-07 before autostart/MCP integration:
nonblocking request handling must be the common service contract, rather than a
list of specially deferred Session methods around a mutable synchronous dispatcher.
Existing functional evidence remains valid for its recorded source snapshot.

That refinement is now locally complete: transport receive/reply routing uses
the common bounded Runtime endpoint, and domain owners handle worker, uv,
document and Project storage completion. Clients do not borrow the whole Runtime
while waiting. Ordering and lifecycle reservations remain at the Project,
Document and Session boundaries. The full gate and actual Host proof pass; see
[Project ownership and P5-07 completion audit](evidence/phase5-project-owner-20260922.md).
P5-08 autostart and P5-09 metrics are next. Rust callers do not replace actual
Desktop/MCP clients or the JAX GPU acceptance requirement.

### Common Runtime entrypoint refinement

The first implementation slice introduces RuntimeHandle/RuntimeOwner and moves
completion ownership out of stdio. All callers use request tickets, internally
unique correlation and bounded unread replies. The public whole-service
`dispatch_async` waiter has been removed; lower-level admission remains for
component tests. Subsequent slices below complete the domain ownership refinement.
The full Windows/WSL gate and actual Host proof pass this slice; see
[endpoint evidence](evidence/phase5-runtime-endpoint-20260922.md). The same-Project
notebook proof also confirms shared Project package origin with isolated variables
and overlapping execution. A reproduced shutdown/input-thread hang was repaired
using nonblocking Linux bridge input; its failure and final proof are retained.

| Wait or ordering boundary | Current state | Remaining action |
|---|---|---|
| uv execution / worker startup / worker inspection | Supervised outside admission; endpoint owns completion; typed Project completions share request-work ownership | Preserve through later adapters |
| Transport reads / replies | Independent tickets; retained partial frames; bounded outstanding replies | Preserve through further domain refactoring and the future Unix adapter |
| Notebook load/save/import/edit and snapshot | Bounded Document owner queue; blocking filesystem jobs are joined in FIFO order outside admission; full gate/Host proof PASS | Preserve accepted-write ownership through later adapters |
| Session ensure validation / execute preparation | Owned document validation and per-notebook preparation FIFO; lightweight identity check precedes startup reservation | Retain lifecycle/Project exclusion and cancellation order through subsequent Project changes |
| Project prepare/reconcile/status/removal | Single filesystem owner and read-only projection; canonical reservations and write-ahead uncertainty; full gate/Host proof PASS | Preserve through later adapters |
| Session control / stale notification | One coalesced pending interrupt and sticky stop/stale intent; no queue-capacity await; full gate/Host proof PASS | Preserve generation isolation through Project integration |

The document barrier tests now hold Document Service work while completing an
unrelated Session status/interrupt through RuntimeHandle. They race edits,
preparation, restart, stop and Project mutation; a cancelled middle preparation
cannot let later execution overtake the first. Accepted writes survive dropped
replies and shutdown refusal. The Project owner now provides the corresponding
storage barrier, uncertainty, reservation and recovery proofs; P5-07 is DONE.
Full verification and actual Host evidence for this slice are recorded in
[document owner evidence](evidence/phase5-document-owner-20260922.md).
Control burst, priority and same-Project survivor proofs plus the final full gate
and actual Host proof are in [control evidence](evidence/phase5-control-intent-20260922.md).

The completed Project ownership slice covers the complete storage lifecycle:

1. One registry owner serializes filesystem work and publishes read-only
   committed/uncertain metadata; mutable managers are not cloned.
2. Project reservations cover preparation through post-uv fingerprint/commit,
   and uncertainty is durable before launch. Completion follows reconciliation.
3. Canonical path resolution and current lifecycle guards prevent registration
   aliases from bypassing Project exclusion.
4. Status probes and removal use the same owner. Failed removal retains identity
   in memory for retry and never deletes Project files.
5. Storage barriers, failed writes, owner failure and EOF are tested alongside
   actual uv/IPython and Windows Host flows. See the recorded completion audit.

## Detailed contract decisions for the first batch

- D01.1: every native command must have its exit status inspected immediately.
  PowerShell preferences inherited from the caller must not weaken this rule.
  A regression must execute the gate with failing stand-in commands; text matching
  alone is not proof. WSL remains mandatory for the production Unix path.
- D02.1: Runtime owns durable diagnostics. Record publication follows the durable
  write attempt. Retrieval does not depend on event subscriptions or a live worker.
  Cleanup still completes if storage fails; the failure is explicitly observable.
  File bounds, retention, atomic writes and error behavior will be recorded in the
  relevant runtime/observability contract before integration.
- D03.1: a Session owns an OS process group created at launch. Stop/restart/crash
  cleanup covers normal descendants, including when the group leader already
  exited. This is lifecycle ownership, not malware-grade containment of deliberately
  detached workloads. Signal scope must never include another Session or Runtime.

## Risks and external dependencies

Phase 6 local gate: E-MCP-SUPERVISION-20260922 combines all fifteen actual HTTP
MCP tools, Human-origin shared primary/value/FIFO, independent Notebook execution,
authentication/bounds/security contracts and Runtime-owned crash/hang recovery.
Final Windows/WSL and actual Host proofs pass after worker environment inheritance
and pre-hello startup-event repairs. P6-GATE is locally DONE; #32 continues with
Phase 7 Secret Store/handoff/tunnel work. D07 remains incomplete until actual Codex
connection is verified. Actual Desktop/Web ChatGPT, managed identities, installation
and JAX GPU acceptance remain required.

Phase 7 checkpoint: E-SECRET-STORE-20260922 completes native Windows credential
storage and Host configuration IPC with actual Credential Manager/Named Pipe and
full Windows/WSL verification. Secret handoff and tunnel lifecycle remain next.
The official Linux tunnel-client v0.0.14 archive was hash-verified and probed via
version/help without credentials; this is not remote connection acceptance.

Phase 7 tunnel checkpoint: E-TUNNEL-SUPERVISION-20260922 closes P7-03..05 locally.
The normal state model reconciles desired and owned incarnations; lifecycle tests,
full Windows/WSL gate and actual pinned-client/local-provider -> real MCP/Session
proof pass, including provider outage/reconnect and local execution survival.
P7-02 Windows credential handoff and desired-state recovery remain next, followed
by isolation/redaction and actual external-client acceptance. No mock-provider
proof closes P7-GATE or the final Desktop/Codex/Web ChatGPT goal.

- The initial checkout has no Cargo.lock. WSL has Rust 1.88, Python 3.10 and no uv
  on PATH; native Windows has Rust 1.91 and uv 0.12.11. Required versions and
  dependency availability must be established, not inferred from old evidence.
- ChatGPT transport/auth support and account configuration must be verified against
  current official documentation before D08. Actual client authorization may need
  the user's interactive action; no substitute proof closes the final goal.
- Existing projects, notebooks, WSL distributions and credentials are preserved.
  Reviewable installation artifacts and non-destructive tests precede any request
  for a separately authorized destructive legacy reset.
- GPU/WSL and actual external-client results must be reported separately from
  unit/component results. A skipped or unavailable check is not PASS.

## Progress reporting

Record completed work and exact verification evidence in this plan and the existing
ledger as appropriate. Keep the goal active until the agreed real-client outcome
is demonstrated. Do not interpret equal-weight WBS counts as product completeness.

## D08 Host credential handoff checkpoint — 2026-09-22

P7-02 is locally DONE: the Host serializes credential changes and reconciles durable
remote intent with the current Runtime generation. Native Credential Manager/Named
Pipe tests, actual Host -> WSL -> pinned tunnel-client against a local mock provider,
independent process/Notebook checks and the full Windows/WSL gate pass. See
[Host handoff evidence](evidence/phase7-host-handoff-20260922.md).

Next is P7-06 managed isolation/redaction, including package-build processes. The
development distro uses one Linux UID; absence of keys in worker environment/argv
is not proof against same-UID inspection. Actual external clients, Desktop, managed
installation/update and JAX GPU acceptance remain open.

## D08 workload environment and isolation prerequisite — 2026-09-22

Package uv launches now use the same cleared Project environment policy as workers.
Actual build hooks and children, worker and child, Project CLI creation, package
import and persistent saved execution pass the full Windows/WSL gate. See
[workload environment evidence](evidence/phase7-workload-environment-20260922.md).
P7-06 stays IN_PROGRESS for managed identity/file/procfs/IPC isolation and diagnostics.

The [targeted contract audit](evidence/phase7-isolation-contract-audit-20260922.md)
found that the identity launch primitive cannot wait for the later installer gate.
Phase 7 must validate the boundary including owned cleanup; Phase 9 provisions the
same contract automatically. Actual remote clients and remaining delivery packages
retain their completion requirements.

## D08 managed workload launch primitive — 2026-09-22

D-027 preserves the existing Runtime-owned process lifecycle while launching uv
builds and workers under one distinct, unprivileged workload identity. Actual
file/procfs/control-socket denial, Project document access, interrupt/stop/restart/
crash evidence and package-cancellation descendant cleanup pass in a disposable
different-UID fixture. See [identity evidence](evidence/phase7-workload-identity-20260922.md).
No actual external provider or installed managed distro is implied. P7-06 remains
open for complete secret-redaction/diagnostic validation and boundary review.

## D08 diagnostic and credential boundary closure — 2026-09-22

P7-06 and P7-A-02 pass their local criteria; see
[diagnostic/boundary evidence](evidence/phase7-diagnostics-20260922.md).
Runtime diagnostics exclude private text by typed projection. Actual different-UID
uv builds/workers, native Host diagnostics and pinned tunnel process outage/recovery
pass, together with the full Windows/WSL gate. Next is secure connection setup and
actual Codex/Web ChatGPT shared-Session proof. P7-GATE remains open; no actual
external client, Desktop, installed managed WSL, full ZIP or JAX proof is inferred.

## D08 human setup and real-key prerequisites — 2026-09-22

Native Windows credential input and the typed private Host client are implemented;
the complete Windows/WSL gate passes. See
[setup evidence](evidence/phase7-remote-setup-20260922.md) and the
[connection runbook](REMOTE_CONNECTION.md). Native dialog visual acceptance and
actual Codex/Web ChatGPT proof remain open. The native prompt supports keys up to
255 ASCII characters; the full 2048-byte input range remains a Desktop obligation.

D-028 corrects a prerequisite omission: the dedicated WSL environment needed for
real provider credentials must exist before real remote acceptance. Next implement
the minimal reusable bootstrap provisioning/validation path, validate identities,
drive/interop restrictions and actual Host launch target with synthetic inputs,
then connect actual clients. The full installer, Desktop and final acceptance
remain their existing phases; no parent gate is bypassed or counted complete.

## D08 dedicated environment prerequisite — 2026-09-22

D-028 image/import and effective workload restrictions pass on a fresh owned WSL
distro, including actual Host/Notebook execution, fail-closed launch and restart.
The complete Windows/WSL regression gate passes. See
[managed-image evidence](evidence/phase7-managed-image-20260922.md).
Actual account/client acceptance remains P7-A-01; Desktop, installer/update/repair
and JAX acceptance remain open. This does not change the 131/210 DONE count.

## D08 actual shared reads — 2026-09-23

Actual Web ChatGPT and Codex connector calls both read the Host-created value
123456789 from the original primary Session, worker PID 95/generation 1. This
closes P7-A-01's read criterion; the ledger is 132/210 DONE. See the
[real-client checkpoint](evidence/phase7-real-client-20260922.md).
Web ChatGPT's execute-cell attempt was rejected by its safety check without an
operation ID or approval UI. The user is checking directly; actual execution and
P7-GATE remain open. Desktop implementation remains the next phase after that
gate, and final Desktop sharing, installer/update/repair and JAX remain required.
