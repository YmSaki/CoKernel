# CoKernel v1 Work Status / WBS Execution Ledger

Purpose: this file is the executable work-management ledger for `cokernel-v1`. Requirements remain authoritative in `IMPLEMENTATION_ORDER.md`, the other `docs/v1/*` specifications, and Issues #26-#35. This ledger translates them into trackable work items.

## Operating rules

- A Phase is an ordering/gate boundary, not the unit of work.
- Every actionable row has an ID, work package, dependency, completion condition, status, and evidence field.
- `DONE` means the stated completion condition has been proven and evidence is recorded.
- `READY_VERIFY` means implementation/test code exists but the required local/WSL/Windows evidence for closure is not yet recorded.
- `IN_PROGRESS` means work is actively incomplete.
- `TODO` means unblocked/next work not yet completed.
- `BLOCKED` means a predecessor gate or required environment prevents starting the item.
- Never mark an item `DONE` from code presence alone. A failing verification creates or reopens the affected implementation item and records the failure evidence.
- GitHub Actions / GitHub-hosted CI are not part of the canonical v1 validation path.
- Resume work from the first unfinished item in **Execution order** unless new evidence creates a higher-priority blocker.
- One development run targets about **five actionable work items**, including implementation, regression tests, execution and evidence updates. Do not stop after one small fix or count five commands/commits as five completed deliverables.
- If an environment blocks a group, record the blocker once and continue independent verification within the active Phase. Do not repeatedly add preflight-only commits, invent work to fill the batch, weaken a gate, or cross a blocked Phase boundary.
- Component evidence closes only the stated component criterion. It does not close the Rust/WSL integration, Session acceptance, or a parent gate.
- Recompute counts with `python3 scripts/v1/validate_work_status.py --check` whenever task states change.

## Current progress

- Total tracked work items: **210**
- DONE: **132**
- READY_VERIFY: **0**
- IN_PROGRESS: **1**
- TODO: **0**
- BLOCKED: **77**
- Ledger DONE ratio (`DONE / total`, equal-weight rows, not effort or product completeness): **62.9%**
- Phase gates completed: **7 / 12 phases = 58.3%** (Phase 0, 1, 2 inherited; Phases 3, 4, 5 and 6 local evidence; lower-layer repairs revalidated)
- Active Phase: **Phase 7 / #32**
- Phase 3: **43 / 43 implementation rows DONE; 10 / 10 verification rows DONE; 4 / 4 Session acceptance rows DONE; local gate DONE**.

The worker, Session Supervisor and Host/WSL bridge are verified on the local Windows/WSL machine. The canonical gate, real-worker execution, durable failure evidence, ordinary descendant cleanup and Phase 5 Host acceptance pass. The original 24 DONE rows are inherited from #27/#28. Desktop, actual Codex/Web ChatGPT clients, managed-WSL deployment and JAX GPU acceptance remain later-phase work; simulated caller origins are not actual-client proof.

Dedicated connection environment: [E-MANAGED-IMAGE-20260922](evidence/phase7-managed-image-20260922.md). Fresh owned WSL import, actual Host/worker execution, effective restrictions, fail-closed launch, distro restart and full Windows/WSL gate PASS. This closes D-028's environment prerequisite, not actual clients, Desktop, full installer or JAX.

## Execution order

1. `P7-GATE` — actual Web ChatGPT and Codex same-primary reads pass P7-A-01. Web ChatGPT's execution was immediately rejected by its safety check; no operation ID or approval UI was returned. The user is checking execution directly. Preserve the live Session, honest annotations and revision 2; do not retry the rejected action through another path.
2. Complete actual-client execution evidence and review Phase 7 before Desktop implementation. P7-06 and P7-A-02 are locally DONE; the corrected fresh image passes import, verified TLS and Host execution. Managed installation, Desktop and full diagnostic ZIP remain later gates.

Real connection: [checkpoint](evidence/phase7-real-client-20260922.md), updated 2026-09-23. Native credential entry, provider CONNECTED, 15-tool discovery and actual Web ChatGPT/Codex shared-value reads pass. Codex sees the original worker PID 95/generation 1/value 123456789; ai-check is unexecuted. Actual-client execution remains open. The fresh image includes verified public CA roots for Go and Python HTTPS.

Current checkpoint: [E-DIAGNOSTICS-20260922](evidence/phase7-diagnostics-20260922.md). Typed diagnostic projection, actual different-UID build/worker denial, 147 secret-free public frames, native Host/WSL diagnostics, tunnel disconnect-survival and the complete Windows/WSL gate PASS. The remote service in these tests is local and simulated; actual Codex/Web ChatGPT acceptance remains open. Earlier notes below retain their checkpoint-specific scope.

Component work: [E-WORKLOAD-IDENTITY-20260922](evidence/phase7-workload-identity-20260922.md). One managed launch policy now separates Runtime and workload UIDs for both builds and Sessions while retaining Runtime supervision. Actual denial and lifecycle proof pass; P7-06 remains open for complete redaction/diagnostics and boundary review. Installed managed WSL and actual clients remain separate gates.

Component work: [workload environment boundary](evidence/phase7-workload-environment-20260922.md) closes inherited control environment exposure in uv build hooks using the same policy as workers. P7-06 remains IN_PROGRESS. The [isolation contract audit](evidence/phase7-isolation-contract-audit-20260922.md) defines remaining identity/IPC/filesystem/cleanup proof and clarifies Phase 7 versus installer responsibilities.

Evidence: [E-HOST-HANDOFF-20260922](evidence/phase7-host-handoff-20260922.md). Controlled Windows credential handoff, durable independent remote intent, rotation, Host/Runtime recovery and confirmed cleanup pass native tests, actual Windows Host/WSL/pinned tunnel-client with a local mock provider, and the complete Windows/WSL gate. Worker identity/value survive enable and credential removal. Managed identity isolation and actual external clients remain open; Phase 7 gate is not complete.

Evidence: [E-TUNNEL-SUPERVISION-20260922](evidence/phase7-tunnel-supervision-20260922.md). P7-03..05 Runtime lifecycle/order/recovery pass normal-state reconciliation tests, real child supervision, full Windows/WSL gate and pinned actual tunnel-client -> local mock provider -> actual MCP/primary Session. Provider disconnect/reconnect preserves local computation and worker identity. Windows credential handoff, managed identities and real external clients remain open; Phase 7 gate is not complete.

Evidence: [E-SECRET-STORE-20260922](evidence/phase7-secret-store-20260922.md). P7-01 Windows credential storage and metadata-only Host IPC pass actual native API/Named Pipe tests and the full Windows/WSL gate. Official tunnel-client v0.0.14 was hash-verified and its version/help inspected without credentials or a running tunnel. Controlled handoff, remote supervision and actual client proof remain open.

Evidence: [E-MCP-SUPERVISION-20260922](evidence/phase6-mcp-supervision-20260922.md). Actual Runtime-owned recovery, retry exhaustion/manual recovery, hung-service health, stable endpoint/bearer and surviving Notebook namespace pass. Full Windows/WSL and actual Host proofs revalidate worker environment isolation and startup-event handshake repairs. Phase 6 is locally DONE. Actual Desktop/Codex/Web ChatGPT, managed identities and JAX GPU remain required.

Earlier checkpoint: [E-MCP-TOOLS-20260922](evidence/phase6-mcp-tools-20260922.md). All 15 tools, exact contracts/bounds, actual HTTP shared-state/FIFO, independent Notebook execution, hostile inspection and Session controls pass. The full gate exposed early-interrupt/empty-cell null execution counts and inherited Runtime cwd; both were repaired and Phase 3 revalidated. P6-05..08 are locally DONE; P6-10 and parent gates remain open.

Earlier checkpoint: [E-MCP-FOUNDATION-20260922](evidence/phase6-mcp-foundation-20260922.md). P6-01..04 local service, official SDK transport, bearer and private Runtime connection pass the full Windows/WSL gate and actual process crash/restart proof. Two read tools exist; the minimum tool surface, automatic supervised recovery, managed identity isolation and actual Desktop/Codex/Web ChatGPT acceptance remain open. P6-10 makes the existing MCP_TUNNEL section 17 recovery requirement explicit in this ledger.

Evidence: [E-METRICS-20260922](evidence/phase5-metrics-20260922.md). Resource collection/aggregation passes component tests, the full Windows/WSL gate, real RTX 5090 readings, independent OS/NVIDIA references and actual Host/Notebook proof. The requirement-by-requirement audit closes P5-09, P5-11, P5-A-01 and P5-GATE locally. No Desktop/MCP/JAX or real Windows sign-in acceptance is implied.

Evidence: [E-AUTOSTART-20260922](evidence/phase5-autostart-20260922.md). P5-08 registration/argument/isolation tests, actual background Host STOPPED/RUNNING/reconnect, full Windows/WSL gate and actual notebook proof PASS. Real sign-in and installer acceptance remain open. A pre-existing fake-worker startup mode defect was repaired and its restart/stop regression strengthened.

P5-07 is locally DONE after the common Runtime, document ownership, control and
Project ownership refinements. Evidence notes below describe their chronological
checkpoints; current task rows and this execution order govern remaining work.

Evidence: [E-NB-RUNTIME-20260921](evidence/phase4-runtime-20260921.md). Phase 4's real imported-notebook execution/persistence gate passes; actual Desktop/MCP/JAX GPU acceptance remains required.

Evidence: [E-HOST-CORE-20260921](evidence/phase5-host-core-20260921.md). Host startup/bridge/handshake/desired state/recovery components pass, including real Windows Host -> WSL -> worker. This is not Named Pipe/Desktop/MCP acceptance and does not close P5-GATE.

Evidence: [E-HOST-PIPE-IMPORT-20260921](evidence/phase5-pipe-import-20260921.md). Current-user Named Pipe and Windows chunked import pass through actual WSL/uv/IPython, including multiple clients, accepted execution after disconnect, saved-file validation and worker exit. Host-only snapshots do not close P5-07; Rust clients do not replace Desktop/MCP acceptance.

Evidence: [E-HOST-EVENTS-20260921](evidence/phase5-events-20260921.md). Runtime snapshots and live execution/output/completion events pass through actual Named Pipe/WSL without polling. Early-interrupt worker failure was reproduced and repaired; the complete Windows/WSL gate passes. Package dispatch was still synchronous at that checkpoint.

Evidence: [E-ASYNC-PACKAGES-20260921](evidence/phase5-async-packages-20260921.md). Package add/remove/sync now return operation IDs while Session control stays responsive. Cancellation/EOF recovery, persistent uncertainty and registry-write-failure repair pass the full gate; actual Host observes package completion without polling. Project create/register and long Session startup/inspection keep P5-07 IN_PROGRESS.

Evidence: [E-PROJECT-SETUP-20260922](evidence/phase5-project-setup-20260922.md). Project create/register now return operation IDs; cancellation preserves files and sync resumes incomplete initialization across Runtime restart. The full gate and real Windows Host proof pass. Long Session startup/inspection dispatch is the remaining P5-07 work.

Evidence: [E-SESSION-RPC-20260922](evidence/phase5-session-rpc-20260922.md). Bounded concurrent replies, deduplicated primary startup, independent notebook startup, control during pending inspection, lifecycle exclusion and cancellation/cleanup pass actual uv/IPython, full Windows/WSL gate and real Host checks. P5-07 was then reopened by user architecture review: the common admission/completion interface still needs refinement. Earlier test results remain valid for their recorded snapshot; Desktop/MCP/JAX GPU acceptance remains open.

Evidence: [E-RUNTIME-ENDPOINT-20260922](evidence/phase5-runtime-endpoint-20260922.md). Common client tickets/lifetime/correlation and nonblocking Linux bridge input pass the full Windows/WSL gate and actual Host proof. Same-Project notebooks share Project packages while keeping distinct namespaces and overlapping execution. P5-07 remains IN_PROGRESS for domain storage waits and generic completion scheduling.

Evidence: [E-DOCUMENT-OWNER-20260922](evidence/phase5-document-owner-20260922.md). Document storage, snapshots and execution preparation now use owned FIFO work and common reply scheduling. Stalled storage leaves unrelated Session status/interrupt responsive; preparation cancellation, lifecycle exclusion and shutdown refusal preserve accepted writes. Full Windows/WSL gate and actual Host proof PASS. Project registry/control queue waits remain; P5-07 stays IN_PROGRESS.

Evidence: [E-CONTROL-INTENT-20260922](evidence/phase5-control-intent-20260922.md). Session control no longer waits for queue slots; interrupt coalescing and sticky stop/stale intent keep memory bounded. A non-yielding burst proof, surviving same-Project Notebook, full gate and actual Host proof PASS. Project registry storage remains the next P5-07 work.

Evidence: [E-PROJECT-OWNER-20260922](evidence/phase5-project-owner-20260922.md). Owned Project preparation/reconciliation/status/removal, canonical alias reservations, failed-write retry and EOF without a late uv launch pass component tests, the full gate and actual Host proof. The completion audit closes P5-07 locally. Phase 5 and real Desktop/MCP/JAX acceptance remain open.

Evidence: [E-IMPORT-OUTPUT-20260921](evidence/phase4-import-output-20260921.md).

Evidence: [E-DOC-20260921](evidence/phase4-documents-20260921.md).

Evidence: [E-LOCAL-20260921](evidence/phase3-local-20260921.md), [E-WORKER-FULL](evidence/phase3-worker-full-e7b208f.md), [E-BATCH](evidence/phase3-worker-components-946d479.md), [E-FIX](evidence/phase3-protocol-fixtures-9a22667.md), [E-RUNTIME-INSP](evidence/phase3-runtime-inspection-51e50e5.md).

## Phase summary

| Phase | Issue | Items | DONE | READY_VERIFY | IN_PROGRESS | TODO | BLOCKED | State |
|---|---:|---:|---:|---:|---:|---:|---:|---|
| 0 | #27 | 8 | 8 | 0 | 0 | 0 | 0 | DONE |
| 1-2 | #28 | 16 | 16 | 0 | 0 | 0 | 0 | DONE |
| 3 | #29 | 58 | 58 | 0 | 0 | 0 | 0 | DONE (local gate) |
| 4 | #30 | 16 | 16 | 0 | 0 | 0 | 0 | DONE (local gate) |
| 5 | #31 | 14 | 14 | 0 | 0 | 0 | 0 | DONE (local gate) |
| 6 | #32 | 12 | 12 | 0 | 0 | 0 | 0 | DONE (local gate) |
| 7 | #32 | 9 | 8 | 0 | 1 | 0 | 0 | ACTIVE |
| 8 | #33 | 17 | 0 | 0 | 0 | 0 | 17 | BLOCKED |
| 9 | #34 | 11 | 0 | 0 | 0 | 0 | 11 | BLOCKED |
| 10 | #34 | 12 | 0 | 0 | 0 | 0 | 12 | BLOCKED |
| 11 | #35 | 37 | 0 | 0 | 0 | 0 | 37 | BLOCKED |

Umbrella #26 remains open until Phase 11 release acceptance closes.

## Phase 0 — Technical spikes and repository reset (#27)

| ID | Work package | Work item | State | Depends on | Done when | Evidence / note |
|---|---|---|---|---|---|---|
| P0-01 | Repository | Create v1 Cargo workspace/repository skeleton | DONE | none | Workspace exists and builds as v1 skeleton | #27 closed |
| P0-02 | Repository | Keep v0.1 as reference without v1 core dependency | DONE | P0-01 | v1 core does not depend on legacy runtime | #27 closed |
| P0-03 | uv spike | Prove Project uv environment + product-controlled worker overlay launch | DONE | P0-01 | Project packages visible; worker tooling not added to Project dependency truth | #38 closed |
| P0-04 | IPython spike | Prove persistent IPython namespace | DONE | P0-01 | Two executions share namespace | #37 closed |
| P0-05 | IPython spike | Prove expression/stdout/stderr/rich MIME/top-level async/interrupt | DONE | P0-04 | Spike tests demonstrate required execution semantics | #37 closed |
| P0-06 | Windows IPC spike | Prove Tauri/Host Named Pipe integration approach | DONE | P0-01 | Named Pipe approach documented/proven | #27 closed |
| P0-07 | MCP spike | Confirm Rust MCP Streamable HTTP/annotation implementation approach | DONE | P0-01 | Implementation path documented/proven | #27 closed |
| P0-GATE | Gate | Pass Phase 0 feasibility gate | DONE | P0-03..07 | All risky feasibility spikes complete | #27 closed |

## Phases 1–2 — Domain/protocol core + uv Project manager (#28)

| ID | Work package | Work item | State | Depends on | Done when | Evidence / note |
|---|---|---|---|---|---|---|
| P12-01 | Domain | Implement IDs and core domain structs | DONE | P0-GATE | Domain types compile and are covered by tests | #28 closed |
| P12-02 | Domain | Implement Runtime/Project/Environment/Notebook/Session/Operation/Failure states | DONE | P12-01 | State models and transitions exist | #28 closed |
| P12-03 | Domain | Implement structured errors/error codes | DONE | P12-01 | Errors are typed and mappable across protocol | #28 closed |
| P12-04 | Protocol | Implement framed JSON codec | DONE | P12-03 | Encode/decode framed JSON with bounds | #28 closed |
| P12-05 | Protocol | Implement version handshake/negotiation | DONE | P12-04 | Version match/mismatch behavior tested | #28 closed |
| P12-06 | Protocol | Add malformed-frame/fuzz/bounds tests | DONE | P12-04 | Malformed/oversized frames rejected | #28 closed |
| P12-07 | Protocol | Add protocol fixture validation | DONE | P12-04 | Fixtures validate against protocol contract | #28 closed |
| P12-08 | Runtime | Create Linux cokernel-runtime skeleton | DONE | P12-01..05 | Runtime CLI starts and exposes Project slice | #28 closed |
| P12-09 | Project | Implement Project registry | DONE | P12-08 | Projects can be registered/resolved | #28 closed |
| P12-10 | Project | Implement Project create/open/list | DONE | P12-09 | CLI operations work on temporary Projects | #28 closed |
| P12-11 | uv environment | Implement uv invocation wrapper | DONE | P12-08 | Runtime invokes uv deterministically | #28 closed |
| P12-12 | uv environment | Implement package add/remove/sync | DONE | P12-11 | Project dependency operations update environment | #28 closed |
| P12-13 | Environment | Implement status/generation/stale semantics | DONE | P12-12 | Generation/status changes are tracked | #28 closed |
| P12-14 | Safety | Implement Project path containment/safety | DONE | P12-10 | Path escape is rejected | #28 closed |
| P12-15 | Verification | Pass domain/protocol/Project component tests | DONE | P12-01..14 | Required component tests pass | #28 closed |
| P12-GATE | Gate | Pass temporary Project package import gate | DONE | P12-15 | Created/synced Project imports installed package | #28 closed |

## Phase 3 — Python/IPython worker + Session Supervisor (#29)

| ID | Work package | Work item | State | Depends on | Done when | Evidence / note |
|---|---|---|---|---|---|---|
| P3-WRK-01 | Worker | Production cokernel_worker package + private Unix socket loop | DONE | P12-GATE | Worker serves framed requests over private Unix socket | E-WORKER-FULL: separate python -m cokernel_worker over Supervisor-owned AF_UNIX socket PASS |
| P3-WRK-02 | Worker | ready/heartbeat/ping/reset/shutdown protocol | DONE | P3-WRK-01 | Lifecycle control frames behave per contract | E-WORKER-FULL: full suite + source subprocess ready/reset/shutdown PASS |
| P3-WRK-03 | Worker | Worker frame shape/version/size validation | DONE | P3-WRK-01 | Malformed/oversized frames fail closed | E-WORKER-FULL: frame/version/type/session/id/method/payload rejection + survival PASS |
| P3-WRK-04 | Worker | Preserve JSON semantics across worker transport | DONE | P3-WRK-03 | Lossy non-string object keys and invalid values rejected | E-WORKER-FULL: integer/key/non-finite JSON boundaries PASS |
| P3-WRK-05 | Worker | Normalize user exception names at protocol boundary | DONE | P3-WRK-03 | ename is bounded/non-empty without user hooks | E-WORKER-FULL: bounded + nameless exception serialization PASS |
| P3-EXE-01 | Execution | Persistent IPython namespace | DONE | P3-WRK-01 | State survives sequential executions | E-BATCH: test_namespace_persists_across_cells PASS |
| P3-EXE-02 | Execution | Capture stdout/stderr | DONE | P3-EXE-01 | Streams emitted as bounded output events | E-WORKER-FULL: 7MB stdout/stderr become bounded ordered WorkerLoop events |
| P3-EXE-03 | Execution | Capture final expression result | DONE | P3-EXE-01 | Expression result returned with MIME data | E-BATCH: final-expression, history and semicolon tests PASS |
| P3-EXE-04 | Execution | Capture explicit rich MIME display | DONE | P3-EXE-01 | Rich display normalized and bounded | E-WORKER-FULL: HTML/binary/SVG normalization + blob limits PASS |
| P3-EXE-05 | Execution | Support top-level async | DONE | P3-EXE-01 | Supported await execution completes correctly | E-BATCH: top-level await PASS |
| P3-EXE-06 | Execution | Convert Python exceptions to structured FAILED outcomes | DONE | P3-EXE-01 | User exception does not crash worker/session | E-WORKER-FULL: structured failures + same-process namespace survival PASS |
| P3-EXE-07 | Execution | Bound output and produce truncation accounting | DONE | P3-EXE-02..04 | Output limits/truncation metadata produced | E-WORKER-FULL: event/operation/blob/stream bounds and omitted-byte/reason accounting PASS |
| P3-EXE-08 | Execution | Contain output normalization/transport failures | DONE | P3-EXE-04 | Bad rich output becomes FAILED outcome and worker survives | E-WORKER-FULL: normalization/invalid-JSON failure + persistent worker survival PASS |
| P3-INSP-01 | Inspection | Implement list_variables | DONE | P3-EXE-01 | Lists safe/coarse variable metadata | E-BATCH: list/support/metadata tests PASS |
| P3-INSP-02 | Inspection | Implement get_variable | DONE | P3-INSP-01 | Returns bounded exact built-in safe values | E-BATCH: exact built-in, integer, identifier and rejection tests PASS |
| P3-INSP-03 | Inspection | Reject hostile/custom object behavior | DONE | P3-INSP-01..02 | No repr/str/iter/getattr/operator hooks invoked | E-BATCH: identity-only dispatch; hostile/metaclass regressions PASS |
| P3-INSP-04 | Inspection | Bound inspection depth/items/string/response size | DONE | P3-INSP-02 | Inspection cannot generate unbounded response | E-BATCH: complete get/list result budget and exact boundaries PASS |
| P3-INSP-05 | Inspection / Runtime boundary | Revalidate worker inspection results fail-closed before exposing callers | DONE | P3-INSP-01..04 | Runtime rejects malformed, duplicate, oversized or internally inconsistent list/get results and preserves explicit Python None semantics | E-LOCAL-20260921: Linux check.sh / real Supervisor / inspection / component tests PASS |
| P3-SUP-01 | Supervisor | Own one worker process per Session | DONE | P3-WRK-01 | Each Session has independent worker/process ownership | E-LOCAL-20260921: Linux check.sh / real Supervisor / inspection / component tests PASS |
| P3-SUP-02 | Supervisor | Private listener/socket + ready handshake | DONE | P3-SUP-01 | Runtime establishes and validates worker readiness | E-LOCAL-20260921: Linux check.sh / real Supervisor / inspection / component tests PASS |
| P3-SUP-03 | Supervisor | Per-Session FIFO single-flight queue shared by callers | DONE | P3-SUP-02 | Human/AI operations serialize in accepted order | E-LOCAL-20260921: Linux check.sh / real Supervisor / inspection / component tests PASS |
| P3-SUP-04 | Supervisor | Interrupt active execution | DONE | P3-SUP-03 | Interrupt terminates active operation without corrupting Session | E-MCP-TOOLS-20260922: IPython-caught early interrupt/empty-cell null-count regression reproduced and repaired; numeric wire lifecycle, real Supervisor and full gate PASS |
| P3-SUP-05 | Supervisor | Graceful stop | DONE | P3-SUP-02 | Session stops cleanly and reports state | E-LOCAL-20260921: Linux check.sh / real Supervisor / inspection / component tests PASS |
| P3-SUP-06 | Supervisor | Restart Session | DONE | P3-SUP-05 | Worker restarts with explicit Session semantics | E-LOCAL-20260921: Linux check.sh / real Supervisor / inspection / component tests PASS |
| P3-SUP-07 | Supervisor | Heartbeat timeout/liveness semantics | DONE | P3-WRK-02,P3-SUP-02 | Lost/stale worker is detected deterministically | E-LOCAL-20260921: Linux check.sh / real Supervisor / inspection / component tests PASS |
| P3-SUP-08 | Supervisor | Crash containment | DONE | P3-SUP-01 | Worker crash does not kill Runtime or other Sessions | E-LOCAL-20260921: Linux check.sh / real Supervisor / inspection / component tests PASS |
| P3-SUP-09 | Supervisor | FailureRecord capture | DONE | P3-SUP-08 | Crash records exit/failure evidence and last operation | E-LOCAL-20260921: Linux check.sh / real Supervisor / inspection / component tests PASS |
| P3-SUP-10 | Supervisor | Environment generation/stale marking | DONE | P12-13,P3-SUP-01 | Existing Session stays alive but becomes stale after env mutation | E-LOCAL-20260921: Supervisor stale notification preserves live memory; public Project mutation wiring remains D06 |
| P3-SUP-11 | Supervisor | Concurrent independent Sessions | DONE | P3-SUP-01 | A/B overlap and namespaces remain isolated | E-LOCAL-20260921: Linux check.sh / real Supervisor / inspection / component tests PASS |
| P3-SUP-12 | Failure evidence | Durable bounded failure store and observable persistence errors | DONE | P3-SUP-09 | Failure survives no subscriber/reopen/restart; persistence failure remains visible | E-LOCAL-20260921: failure-store and process tests PASS |
| P3-SUP-13 | Process ownership | Cleanup worker process group on stop/restart/crash/startup failure | DONE | P3-SUP-01 | Ordinary descendants exit; unrelated Session survives | E-LOCAL-20260921: real descendant process tests PASS |
| P3-SUP-14 | Worker launch | Project-root initial working directory | DONE | P3-SUP-01 | Relative notebook file operations resolve in the Project rather than Runtime control directory | E-MCP-TOOLS-20260922: real worker getcwd equals Project root; relative file barrier and full Windows/WSL gate PASS |
| P3-SUP-15 | Worker launch | Explicit workload environment and Project-local profile | DONE | P3-SUP-01 | Product/control variables do not reach worker environment or Runtime-home profile | E-MCP-SUPERVISION-20260922: actual sentinel inheritance failed before repair; explicit worker environment/profile, actual process proof and final full Windows/WSL gate PASS |
| P3-LC-01 | Protocol invariants | Bind worker frames to active transaction | DONE | P3-SUP-03 | Stale/wrong transaction frames fail closed | E-LOCAL-20260921: Linux check.sh / real Supervisor / inspection / component tests PASS |
| P3-LC-02 | Protocol invariants | Require single execution_started before outputs | DONE | P3-LC-01 | Duplicate/missing/out-of-order start rejected | E-LOCAL-20260921: Linux check.sh / real Supervisor / inspection / component tests PASS |
| P3-LC-03 | Protocol invariants | Allow outputs only between start and finish | DONE | P3-LC-02 | Output before start/after finish rejected | E-LOCAL-20260921: Linux check.sh / real Supervisor / inspection / component tests PASS |
| P3-LC-04 | Protocol invariants | Require contiguous output sequence 1..N | DONE | P3-LC-02 | Sequence gaps/duplicates rejected | E-LOCAL-20260921: Linux check.sh / real Supervisor / inspection / component tests PASS |
| P3-LC-05 | Protocol invariants | Validate execution_finished.output_count | DONE | P3-LC-04 | Count equals accepted outputs | E-LOCAL-20260921: Linux check.sh / real Supervisor / inspection / component tests PASS |
| P3-LC-06 | Protocol invariants | Require final response after execution_finished | DONE | P3-LC-05 | Early/missing response rejected | E-LOCAL-20260921: Linux check.sh / real Supervisor / inspection / component tests PASS |
| P3-LC-07 | Protocol invariants | Align status/execution_count between finish and response | DONE | P3-LC-06 | Terminal response agrees with lifecycle | E-LOCAL-20260921: Linux check.sh / real Supervisor / inspection / component tests PASS |
| P3-LC-08 | Protocol invariants | Preflight oversized execute requests | DONE | P3-SUP-03 | Oversized caller request rejected without crashing healthy Session | E-LOCAL-20260921: Linux check.sh / real Supervisor / inspection / component tests PASS |
| P3-LC-09 | Protocol invariants | Bind final result.operation_id exactly to active Runtime operation | DONE | P3-LC-06 | Mismatch is rejected fail-closed with regression test | E-LOCAL-20260921: Linux check.sh / real Supervisor / inspection / component tests PASS |
| P3-LC-10 | Protocol invariants | Validate truncation accounting self-consistency and final-response agreement | DONE | P3-EXE-07,P3-LC-05 | Flags/omitted bytes/reasons are internally consistent and identical at finish/response | E-LOCAL-20260921: Linux check.sh / real Supervisor / inspection / component tests PASS |
| P3-V-01 | Verification | cargo fmt --check | DONE | P3-WRK-01..P3-LC-10 | exit 0 on Rust-equipped checkout | E-LOCAL-20260921: Linux check.sh / real Supervisor / inspection / component tests PASS |
| P3-V-02 | Verification | cargo check --workspace | DONE | P3-WRK-01..P3-LC-10 | exit 0 | E-LOCAL-20260921: Linux check.sh / real Supervisor / inspection / component tests PASS |
| P3-V-03 | Verification | cargo clippy --workspace --all-targets -- -D warnings | DONE | P3-WRK-01..P3-LC-10 | exit 0 with no warnings | E-LOCAL-20260921: Linux check.sh / real Supervisor / inspection / component tests PASS |
| P3-V-04 | Verification | cargo test --workspace | DONE | P3-WRK-01..P3-LC-10 | all tests pass | E-LOCAL-20260921: Linux check.sh / real Supervisor / inspection / component tests PASS |
| P3-V-05 | Verification | worker pytest | DONE | P3-WRK-01..P3-LC-10 | all worker tests pass | E-LOCAL-20260921: Linux check.sh / real Supervisor / inspection / component tests PASS |
| P3-V-06 | Verification | protocol fixture validation | DONE | P3-WRK-01..P3-LC-10 | all fixtures pass | E-LOCAL-20260921: Linux check.sh / real Supervisor / inspection / component tests PASS |
| P3-V-07 | Verification | scripts/v1/check.sh | DONE | P3-V-01..06 | canonical Linux/WSL local gate exits 0 | E-LOCAL-20260921: Linux check.sh / real Supervisor / inspection / component tests PASS |
| P3-V-08 | Verification | scripts/v1/check.ps1 applicable checks | DONE | P3-V-01..06 | canonical Windows-side checks exit 0 | E-LOCAL-20260921: combined Windows/WSL gate PASS; 170-file identity verified before/after; GitHub issue state unchanged |
| P3-V-09 | Verification | Supervisor real-process/socket smoke | DONE | P3-V-07 | start/handshake/queue/interrupt/restart/stop/crash paths pass | E-LOCAL-20260921; E-HOST-EVENTS-20260921: early-interrupt repair and real-process smoke reverified in full gate |
| P3-V-10 | Verification | Inspection hostile-object smoke | DONE | P3-V-07 | safe inspection passes against real worker | E-LOCAL-20260921: Linux check.sh / real Supervisor / inspection / component tests PASS |
| P3-A-01 | Acceptance | Persistent state proof: x=123 then x+1 == 124 | DONE | P3-V-09 | returns 124 in same Session | E-LOCAL-20260921: Linux check.sh / real Supervisor / inspection / component tests PASS |
| P3-A-02 | Acceptance | Parallel A/B Session proof | DONE | P3-V-09 | actual overlap + isolated namespace/PID | E-LOCAL-20260921: Linux check.sh / real Supervisor / inspection / component tests PASS |
| P3-A-03 | Acceptance | Crash containment proof | DONE | P3-V-09 | kill A; B survives; A has FailureRecord | E-LOCAL-20260921: Linux check.sh / real Supervisor / inspection / component tests PASS |
| P3-A-04 | Acceptance | Hostile inspection acceptance | DONE | P3-V-10 | hostile object corpus does not execute user hooks | E-LOCAL-20260921: Linux check.sh / real Supervisor / inspection / component tests PASS |
| P3-GATE | Gate | Close Phase 3 / #29 | DONE | P3-A-01..04,P3-V-08 | All Phase 3 deliverables and canonical gate evidence PASS | E-MCP-TOOLS-20260922: lower gate revalidated after early-interrupt and Project cwd repairs; full Windows/WSL gate PASS |

## Phase 4 — Notebook Document Service (#30)

| ID | Work package | Work item | State | Depends on | Done when | Evidence / note |
|---|---|---|---|---|---|---|
| P4-01 | Notebook model | Implement nbformat v4 Rust model | DONE | P3-GATE | Supported notebooks parse/serialize structurally valid | E-DOC-20260921: 18 new notebook tests PASS; Linux Rust 115 PASS; fixture nbformat validation PASS (domain component only) |
| P4-02 | Notebook model | Preserve unknown notebook/cell metadata | DONE | P4-01 | Round-trip preserves unknown metadata | E-DOC-20260921: 18 new notebook tests PASS; Linux Rust 115 PASS; fixture nbformat validation PASS (domain component only) |
| P4-03 | Document service | Create/open/list notebooks | DONE | P4-01 | Project notebook CRUD read paths work | E-DOC-20260921: 18 new notebook tests PASS; Linux Rust 115 PASS; fixture nbformat validation PASS (domain component only) |
| P4-04 | Document service | Revisioned edits and conflict detection | DONE | P4-03 | Stale revision is rejected without overwrite | E-DOC-20260921: 18 new notebook tests PASS; Linux Rust 115 PASS; fixture nbformat validation PASS (domain component only) |
| P4-05 | Persistence | Atomic save | DONE | P4-03 | Save is atomic and failure-safe | E-DOC-20260921: 18 new notebook tests PASS; Linux Rust 115 PASS; fixture nbformat validation PASS (domain component only) |
| P4-06 | Persistence | External-change detection | DONE | P4-03 | External modification causes explicit conflict | E-DOC-20260921: 18 new notebook tests PASS; Linux Rust 115 PASS; fixture nbformat validation PASS (domain component only) |
| P4-07 | Execution integration | Convert worker events to nbformat outputs | DONE | P3-GATE,P4-01 | Streams/errors/rich MIME persist validly | E-IMPORT-OUTPUT-20260921: Runtime component proofs PASS; public/Host adapters remain integration work |
| P4-08 | Execution integration | Execute cell by authoritative notebook/cell ID | DONE | P4-03,P4-07 | Correct cell executes in Project Session | E-NB-RUNTIME-20260921: real framed RPC/Project/IPython execution plus admission/source-conflict/late-completion regression PASS |
| P4-09 | Import | Windows->WSL notebook import transaction | DONE | P4-03 | Chunk/import transaction handles bytes safely | E-IMPORT-OUTPUT-20260921: Runtime component proofs PASS; public/Host adapters remain integration work |
| P4-10 | Import | Validate import hash/size/path/collision | DONE | P4-09 | Mismatch/path escape/collision rejected | E-IMPORT-OUTPUT-20260921: Runtime component proofs PASS; public/Host adapters remain integration work |
| P4-11 | Compatibility | Build notebook fixture suite | DONE | P4-01 | Required fixtures cover streams/errors/HTML/PNG/SVG/attachments/metadata/large/malformed | E-DOC-20260921: 18 new notebook tests PASS; Linux Rust 115 PASS; fixture nbformat validation PASS (domain component only) |
| P4-13 | Document lifecycle | Integrate explicit reconciliation and editor close/flush | DONE | P4-08 | Close preserves Session and notebook identity; dirty/conflict states require explicit handling | E-NB-RUNTIME-20260921: dirty close flush, same NotebookId/Session reopen, reload/external-conflict and caller-independent persistence PASS |
| P4-14 | Execution delivery | Bounded complete output receipt independent of UI subscriptions | DONE | P3-GATE | Subscriber lag cannot silently lose persisted results; overflow/crash fail explicitly | E-IMPORT-OUTPUT-20260921: 2048 outputs persisted despite listener lag; real IPython capture and overflow tests PASS |
| P4-12 | Verification | Notebook unit/component/fixture tests | DONE | P4-01..11,P4-13,P4-14 | All notebook tests pass | E-NB-RUNTIME-20260921: canonical Windows/WSL gates PASS; 133 Linux/39 Windows Rust tests plus 5-test protocol follow-up per platform |
| P4-A-01 | Acceptance | Import/execute/persist standard .ipynb proof | DONE | P4-12 | Imported notebook executes, persists outputs, remains valid | E-NB-RUNTIME-20260921: real Runtime/uv/IPython framed import, 124/HTML persisted and nbformat 5.11.1 validation PASS |
| P4-GATE | Gate | Close Phase 4 / #30 | DONE | P4-A-01 | All Phase 4 evidence PASS | E-NB-RUNTIME-20260921: combined Windows/WSL gate PASS; 95-file executable source identity; local closure only, GitHub issue unchanged |

## Phase 5 — Windows Host + WSL Runtime bridge (#31)

| ID | Work package | Work item | State | Depends on | Done when | Evidence / note |
|---|---|---|---|---|---|---|
| P5-01 | Host | Create cokernel-host Windows binary/crate | DONE | P4-GATE | Host builds and starts | E-HOST-CORE-20260921: native binary build/serve and independent supervisor PASS |
| P5-02 | Bridge | Own long-lived wsl.exe runtime bridge | DONE | P5-01 | Host owns persistent bridge process | E-HOST-CORE-20260921: actual Windows Host/WSL/Runtime/worker execution, caller drop, acknowledged stop and worker exit PASS |
| P5-03 | Bridge | Protocol handshake/version mismatch handling | DONE | P5-02 | Compatible handshake succeeds; mismatch fails clearly | E-HOST-CORE-20260921: real handshake plus wrong envelope/protocol, malformed/oversized and timeout tests PASS |
| P5-04 | Lifetime | Persist desired RUNNING/STOPPED state | DONE | P5-02 | Host converges runtime to desired state | E-HOST-CORE-20260921: lock/persist/reopen, failed state save, dirty flush, refused stop with retained document connection and explicit reconciliation PASS |
| P5-05 | Recovery | Reconnect/backoff/circuit breaker | DONE | P5-02 | Bridge failures recover boundedly | E-HOST-CORE-20260921: bounded failure attempts, version circuit break, generation/evidence, no request replay and STOPPED no-reconnect PASS |
| P5-06 | IPC | Current-user Named Pipe RPC/events | DONE | P5-01 | Authorized local clients can RPC/subscribe | E-HOST-PIPE-IMPORT-20260921: kernel ACL, peer identity, bounded RPC/status subscriptions, correlation/cancellation and actual WSL reconnect PASS |
| P5-07 | IPC | Event forwarding and request correlation | DONE | P5-03,P5-06 | Runtime events correlate correctly to clients through common bounded admission/completion | E-PROJECT-OWNER-20260922: common endpoint, document/Project ownership, immediate control, correlation/lifecycle and full gate/actual Host PASS; local closure only |
| P5-08 | Autostart | Install/user auto-start behavior | DONE | P5-01 | Host can run independently of Desktop | E-AUTOSTART-20260922: HKCU setting/argument/isolation tests, actual background binary STOPPED/RUNNING/reconnect and full gate PASS; real sign-in remains P11-L6-13 |
| P5-09 | Metrics | Aggregate Windows + WSL metrics | DONE | P5-02 | CPU/RAM/WSL RAM/GPU/VRAM/storage dashboard | E-METRICS-20260922: cached owned sampler, typed availability/scope, bounded diagnostic, actual Host/WSL and independent hardware references PASS; dashboard rendering is P8-10 |
| P5-10 | Import bridge | Stream Windows notebook bytes to Runtime | DONE | P4-GATE,P5-06 | Notebook import works without unsafe path assumptions | E-HOST-PIPE-IMPORT-20260921: Japanese Windows path, multi-chunk transfer, collision/malformed rejection, actual Project execution, nbformat validation and no Windows path forwarding PASS |
| P5-11 | Verification | Host fake-runtime component tests | DONE | P5-01..10 | desired state/reconnect/ACL/events/secrets/UI disconnect tests pass | E-METRICS-20260922: current full corpus including kernel ACL denial, token restoration, bounded failure recovery, autostart and partial metric failures PASS |
| P5-12 | Bridge startup | Validate startup events before hello | DONE | P5-02 | Bounded startup events preserve sequence without failing a valid handshake | E-MCP-SUPERVISION-20260922: before/fix regression, bounded event validation/sequence continuity and actual Host/MCP-only restart PASS |
| P5-A-01 | Acceptance | UI-independent Host/Runtime lifetime proof | DONE | P5-11 | Host keeps Runtime/Sessions alive across client reconnects | E-METRICS-20260922: actual Named Pipe/WSL/IPython shared primary and accepted execution survive client drop; final background Host binary respects persisted desired state |
| P5-GATE | Gate | Close Phase 5 / #31 | DONE | P5-A-01 | All Phase 5 evidence PASS | E-METRICS-20260922 local gate audit; GitHub #31 state unchanged; actual Desktop/MCP/JAX/managed distro and L6 sign-in remain later requirements |

## Phase 6 — CoKernel-native MCP (#32)

| ID | Work package | Work item | State | Depends on | Done when | Evidence / note |
|---|---|---|---|---|---|---|
| P6-01 | MCP service | Create cokernel-mcp service | DONE | P5-GATE | Service starts locally | E-MCP-FOUNDATION-20260922: actual CLI readiness, SIGKILL/restart retain live Runtime/worker/primary/value; Runtime-owned recovery completed by E-MCP-SUPERVISION-20260922/P6-10 |
| P6-02 | Transport | Implement Streamable HTTP | DONE | P6-01 | MCP transport interoperates with client | E-MCP-FOUNDATION-20260922: official SDK initialization/list/call and older protocol negotiation, Host/Origin/body limits PASS |
| P6-03 | Security | Require local bearer auth | DONE | P6-01 | Unauthenticated calls rejected | E-MCP-FOUNDATION-20260922: all-method auth, duplicate/wrong bearer rejection, private persistent credential, process args/env/log absence PASS; managed UID boundary remains deployment acceptance |
| P6-04 | Runtime client | Implement Runtime Unix API client | DONE | P3-GATE,P6-01 | MCP calls Runtime domain API | E-MCP-FOUNDATION-20260922: existing authority, peer UID/private socket, active/stale path safety, framing/correlation/bounds PASS |
| P6-05 | Tool surface | Project/Notebook/Session/Variable/Resource tools | DONE | P6-04 | Minimum v1 domain tool set exists | E-MCP-TOOLS-20260922: all 14 minimum tools plus get_operation implemented and exercised through actual HTTP/uv/IPython |
| P6-06 | Tool contracts | Exact annotations/schema snapshots | DONE | P6-05 | Annotations and schemas match intended capabilities | E-MCP-TOOLS-20260922: shared schema/validation contract and protocol/mcp/tools.json snapshot; exact annotations PASS |
| P6-07 | Bounds | Bound MCP responses and validate inputs | DONE | P6-05 | Huge/invalid requests cannot escape limits | E-MCP-TOOLS-20260922: closed schemas, UUID/revision/identifier bounds, request-ID caps, huge Unicode/image/value response limits and revisioned pages PASS |
| P6-08 | Queue integration | Human + MCP shared Session FIFO semantics | DONE | P3-GATE,P6-04,P6-05 | MCP and Human serialize through same Session queue | E-MCP-TOOLS-20260922: actual Human-origin bridge and MCP same primary/worker/value; explicit-barrier FIFO; independent Notebook overlap/namespace isolation PASS |
| P6-09 | Verification | MCP contract/security suite | DONE | P6-01..08,P6-10 | bearer/tools/inspection/revision/bounds tests pass | E-MCP-SUPERVISION-20260922: final official SDK/auth/contracts/bounds/ownership/revision/hostile inspection plus all actual HTTP tools and supervision PASS; managed UID isolation remains deployment acceptance |
| P6-10 | MCP supervision | Bounded Runtime-owned MCP recovery | DONE | P6-01 | MCP failure recovers boundedly with readiness evidence and no Session interruption | E-MCP-SUPERVISION-20260922: actual Runtime-owned crash/hang recovery, bounded retries/circuit/manual restart, stable port/bearer, surviving worker/namespace and owned cleanup PASS |
| P6-A-01 | Acceptance | Local same-Session secret_from_master proof | DONE | P6-09 | MCP get_variable returns 123456789 with no second worker | E-MCP-SUPERVISION-20260922: actual authenticated MCP reads Human-origin Host/bridge-created 123456789 from the unchanged primary/worker; actual Desktop/Codex/Web clients remain later acceptance |
| P6-GATE | Gate | Close Phase 6 portion of #32 | DONE | P6-A-01 | Local native MCP gate PASS | E-MCP-SUPERVISION-20260922: combined local native MCP gate audit PASS; #32 remains open for Phase 7; managed isolation and actual clients remain required |

## Phase 7 — Secure Tunnel integration (#32)

| ID | Work package | Work item | State | Depends on | Done when | Evidence / note |
|---|---|---|---|---|---|---|
| P7-01 | Secrets | Windows Secret Store abstraction | DONE | P6-GATE | Credentials stored outside worker/runtime-visible channels | E-SECRET-STORE-20260922: actual Windows Credential Manager, native Named Pipe metadata-only configuration, queue/cancellation/bounds, full Windows/WSL gate PASS; no Linux handoff yet |
| P7-02 | Secrets | Controlled credential handoff | DONE | P7-01 | Only tunnel/MCP components receive required secret | E-HOST-HANDOFF-20260922: native credential/pipe recovery, actual Host/WSL/pinned-client local-provider proof and full Windows/WSL gate PASS; worker env/argv exclude keys; hostile same-UID access remains P7-06 |
| P7-03 | Tunnel | Tunnel Supervisor | DONE | P6-GATE | Tunnel process lifecycle is supervised | E-TUNNEL-SUPERVISION-20260922: seven model/security/process tests, full Windows/WSL gate and actual pinned-client/local-provider real-Session proof PASS; Host handoff P7-02 remains open |
| P7-04 | Tunnel | MCP-before-tunnel startup/readiness ordering | DONE | P7-03 | Tunnel starts only after MCP ready | E-TUNNEL-SUPERVISION-20260922: seven model/security/process tests, full Windows/WSL gate and actual pinned-client/local-provider real-Session proof PASS; Host handoff P7-02 remains open |
| P7-05 | Tunnel | Reconnect/backoff and remote-access status | DONE | P7-03 | Tunnel recovers and status is observable | E-TUNNEL-SUPERVISION-20260922: seven model/security/process tests, full Windows/WSL gate and actual pinned-client/local-provider real-Session proof PASS; Host handoff P7-02 remains open |
| P7-06 | Security | Credential isolation/redaction tests | DONE | P7-01..05 | No tunnel/MCP secret reaches worker/log/diagnostics | E-DIAGNOSTICS-20260922: typed diagnostic exclusion corpus, actual distinct-UID build/worker denial, 147 secret-free response/events, local log/failure/notebook scans, native Host/WSL proof and full gate PASS. Full installed system/ZIP remain later gates |
| P7-A-01 | Acceptance | Remote same-Session proof | DONE | P7-06 | Remote MCP reads Human-created value from same Session | E-REAL-CLIENT-20260922 updated 20260923: actual Web ChatGPT tool response and actual Codex connector read Host-created 123456789 in unchanged primary Session/worker PID 95/generation 1; Desktop and actual execution remain open |
| P7-A-02 | Acceptance | Tunnel-disconnect local-survival proof | DONE | P7-06 | Disconnect/restart leaves local execution healthy | E-DIAGNOSTICS-20260922: actual pinned tunnel-client/local mock provider disconnect/reconnect, MCP replacement and process recovery preserve local worker/value; not actual ChatGPT evidence |
| P7-GATE | Gate | Close Phase 7 / #32 | IN_PROGRESS | P7-A-01..02 | All MCP+tunnel evidence PASS | Actual-client execution remains unproven: Web ChatGPT safety rejection; user checking directly. No annotation weakening or alternate-path retry. Read criterion alone does not close this gate |

## Phase 8 — Desktop application (#33)

| ID | Work package | Work item | State | Depends on | Done when | Evidence / note |
|---|---|---|---|---|---|---|
| P8-01 | Desktop shell | Tauri v2 Windows app + TypeScript frontend skeleton | BLOCKED | P5-GATE | Desktop builds/launches | Phase 8 |
| P8-02 | Host client | Named Pipe RPC/event client + reconnect | BLOCKED | P5-GATE,P8-01 | Desktop projects Host domain state | Phase 8 |
| P8-03 | Home | Runtime status + tray | BLOCKED | P8-02 | Home/tray reflect Host state | Phase 8 |
| P8-04 | Projects | Project list/create/open UI | BLOCKED | P8-02 | Project workflow uses Host API | Phase 8 |
| P8-05 | Packages | uv package add/remove/sync UI | BLOCKED | P8-04 | Package operations exposed without terminal | Phase 8 |
| P8-06 | Notebook | Notebook tree/create/import/editor | BLOCKED | P4-GATE,P8-02 | Standard notebooks editable | Phase 8 |
| P8-07 | Execution | Run cell/selected + output streaming/autosave | BLOCKED | P8-06,P3-GATE | Execution drives primary Session | Phase 8 |
| P8-08 | Sessions | Session list/state/queue/interrupt/restart/stop | BLOCKED | P8-02,P3-GATE | Session lifecycle manageable from UI | Phase 8 |
| P8-09 | Environment | Stale-environment UX | BLOCKED | P8-08 | Stale state visible with restart action | Phase 8 |
| P8-10 | Dashboard | CPU/RAM/WSL RAM/GPU/VRAM/storage dashboard | BLOCKED | P5-09,P8-02 | Resource metrics visible | Phase 8 |
| P8-11 | Failures | Crash/failure evidence UI | BLOCKED | P3-SUP-09,P8-02 | Known evidence shown; unknown cause stated explicitly | Phase 8 |
| P8-12 | Remote AI | MCP/tunnel settings/status | BLOCKED | P7-GATE,P8-02 | Remote access can be controlled/observed | Phase 8 |
| P8-13 | Diagnostics | Logs/diagnostics/update/repair entry points | BLOCKED | P8-02 | User can reach diagnostics/product maintenance | Phase 8 |
| P8-14 | UX invariants | No kernel picker; saved document state distinct from live Session | BLOCKED | P8-04..13 | UI preserves notebook-first domain model | UI_SPEC |
| P8-A-01 | Acceptance | Representative local workflows without routine CLI | BLOCKED | P8-01..14 | Core workflows complete through Desktop | Phase 8 gate |
| P8-A-02 | Acceptance | Desktop close/reopen does not kill Sessions | BLOCKED | P8-02,P5-A-01 | Intended Sessions survive UI lifecycle | Phase 8 gate |
| P8-GATE | Gate | Close Phase 8 / #33 | BLOCKED | P8-A-01..02 | All Desktop evidence PASS | #33 closes |

## Phase 9 — Installer/bootstrap + legacy reset (#34)

| ID | Work package | Work item | State | Depends on | Done when | Evidence / note |
|---|---|---|---|---|---|---|
| P9-01 | Installer | Build CoKernelSetup.exe flow | BLOCKED | P8-GATE | Installer starts and drives provisioning | Phase 9 |
| P9-02 | Preflight | Windows/WSL/NVIDIA preflight | BLOCKED | P9-01 | Unsupported/missing prerequisites diagnosed | Phase 9 |
| P9-03 | Bootstrap | WSL enable/update + reboot/resume | BLOCKED | P9-02 | Provisioning resumes deterministically | Phase 9 |
| P9-04 | Legacy | Detect/export/explicit destructive v0.1 reset | BLOCKED | P9-01 | Legacy reset requires explicit user action and preserves export path | Phase 9 |
| P9-05 | Distro | Create fresh managed distro + identities/permissions/hardening | BLOCKED | P9-03 | Managed workload distro created securely | Phase 9 |
| P9-06 | Payload | Install versioned Runtime payload | BLOCKED | P9-05 | Correct payload version installed | Phase 9 |
| P9-07 | Windows apps | Install Host/Desktop + auto-start | BLOCKED | P9-06 | Host/Desktop installed and startup configured | Phase 9 |
| P9-08 | First run | GPU/runtime first-run acceptance | BLOCKED | P9-06 | Managed runtime passes first-run checks | Phase 9 |
| P9-09 | Logs | Setup logs and diagnosable failure paths | BLOCKED | P9-01 | Provisioning failures leave useful evidence | Phase 9 |
| P9-A-01 | Acceptance | Fresh install with no manual WSL/Linux setup | BLOCKED | P9-01..09 | Fresh supported machine reaches usable CoKernel | Phase 9 gate |
| P9-GATE | Gate | Close Phase 9 portion of #34 | BLOCKED | P9-A-01 | Installer/bootstrap gate PASS | Phase 9 |

## Phase 10 — Update/repair/product hardening (#34)

| ID | Work package | Work item | State | Depends on | Done when | Evidence / note |
|---|---|---|---|---|---|---|
| P10-01 | Update | Signed/versioned artifact update model | BLOCKED | P9-GATE | Artifacts have explicit version/signing policy | Phase 10 |
| P10-02 | Compatibility | Host/Runtime compatibility manifest | BLOCKED | P10-01 | Incompatible combinations rejected clearly | Phase 10 |
| P10-03 | Session safety | Active-Session disruption analysis + defer/confirm | BLOCKED | P10-01 | Update never silently kills active Sessions | Phase 10 |
| P10-04 | Migration | Runtime payload migration | BLOCKED | P10-01 | N->N+1 payload migrates deterministically | Phase 10 |
| P10-05 | Repair | Repair convergence | BLOCKED | P9-GATE | Repair restores product components without deleting Projects | Phase 10 |
| P10-06 | Rollback | Rollback metadata | BLOCKED | P10-01 | Previous viable state is identifiable | Phase 10 |
| P10-07 | Uninstall | Uninstall/data-preservation UX | BLOCKED | P9-GATE | User data preservation choice explicit | Phase 10 |
| P10-08 | Reliability | Crash-loop circuit breakers | BLOCKED | P5-GATE | Repeated failures stop boundedly and remain diagnosable | Phase 10 |
| P10-09 | Diagnostics | Redacted diagnostic ZIP | BLOCKED | P7-06,P9-GATE | Bundle contains evidence but no protected secrets | Phase 10 |
| P10-A-01 | Acceptance | N->N+1 update preserves Projects/notebooks and active Session safety | BLOCKED | P10-01..09 | Update gate passes | Phase 10 gate |
| P10-A-02 | Acceptance | Repair preserves Project data | BLOCKED | P10-05 | Repair convergence proven | Phase 10 gate |
| P10-GATE | Gate | Close Phase 10 / #34 | BLOCKED | P10-A-01..02 | All update/repair evidence PASS | #34 closes |

## Phase 11 — Full acceptance and release candidate (#35)

| ID | Work package | Work item | State | Depends on | Done when | Evidence / note |
|---|---|---|---|---|---|---|
| P11-L0 | Acceptance layer | L0 static/schema gate | BLOCKED | P10-GATE | rust/python/frontend/protocol/docs/secrets/script static checks PASS | TEST_ACCEPTANCE L0 |
| P11-L1 | Acceptance layer | L1 unit-test gate | BLOCKED | P10-GATE | domain/notebook/worker/MCP unit suites PASS | TEST_ACCEPTANCE L1 |
| P11-L2 | Acceptance layer | L2 component-contract gate | BLOCKED | P10-GATE | uv/session/host/runtime component suites PASS | TEST_ACCEPTANCE L2 |
| P11-L3 | Acceptance layer | L3 WSL integration gate | BLOCKED | P10-GATE | managed distro/bridge/uv/worker/import/restart/isolation PASS | TEST_ACCEPTANCE L3 |
| P11-E2E-01 | Local E2E | Project + Notebook journey | BLOCKED | P11-L3 | create Project/package/notebook; x=123; x+1=124; valid persisted ipynb | TEST_ACCEPTANCE E2E-01 |
| P11-E2E-02 | Local E2E | Notebook import journey | BLOCKED | P11-L3 | Windows fixture imports/preserves/executes | TEST_ACCEPTANCE E2E-02 |
| P11-E2E-03 | Local E2E | Parallel Sessions journey | BLOCKED | P11-L3 | A/B workloads overlap and namespaces isolate | TEST_ACCEPTANCE E2E-03 |
| P11-E2E-04 | Local E2E | Cross-Project isolation journey | BLOCKED | P11-L3 | different package states remain isolated | TEST_ACCEPTANCE E2E-04 |
| P11-E2E-05 | Local E2E | Environment change/stale journey | BLOCKED | P11-L3 | live Session marked stale; restart sees new dependency | TEST_ACCEPTANCE E2E-05 |
| P11-E2E-06 | Local E2E | Crash evidence journey | BLOCKED | P11-L3 | forced worker crash produces evidence; unrelated Session survives | TEST_ACCEPTANCE E2E-06 |
| P11-E2E-07 | Local E2E | Human/AI shared queue journey | BLOCKED | P11-L3 | accepted operations serialize and B sees A state | TEST_ACCEPTANCE E2E-07 |
| P11-L4 | Acceptance layer | L4 local end-to-end gate | BLOCKED | P11-E2E-01..07 | All local E2E journeys PASS | TEST_ACCEPTANCE L4 |
| P11-MCP-01 | MCP E2E | Local MCP security/tool journey | BLOCKED | P11-L4 | bearer/list/read/ensure/execute/get_variable/conflict/restart/bounds PASS | TEST_ACCEPTANCE L5 |
| P11-TUN-01 | Tunnel E2E | Tunnel remote/reconnect/isolation journey | BLOCKED | P11-MCP-01 | remote call/reconnect/MCP restart/local survival/secret isolation PASS | TEST_ACCEPTANCE L5 |
| P11-L5 | Acceptance layer | L5 MCP/tunnel gate | BLOCKED | P11-MCP-01,P11-TUN-01 | All MCP/tunnel E2E PASS | TEST_ACCEPTANCE L5 |
| P11-L6-01 | Real workstation | Install from CoKernelSetup.exe | BLOCKED | P11-L5 | Install completes | TEST_ACCEPTANCE L6.1 |
| P11-L6-02 | Real workstation | Managed WSL auto-created; no routine manual Linux setup | BLOCKED | P11-L6-01 | Provisioning fully automatic | TEST_ACCEPTANCE L6.2-3 |
| P11-L6-03 | Real workstation | uv Project + representative CPU package | BLOCKED | P11-L6-02 | Project/env/package works | TEST_ACCEPTANCE L6.4-5 |
| P11-L6-04 | Real workstation | PyTorch GPU smoke when supported | BLOCKED | P11-L6-03 | CUDA workload PASS | TEST_ACCEPTANCE L6.6 |
| P11-L6-05 | Real workstation | JAX GPU computation in target Project Session | BLOCKED | P11-L6-03 | GPU discovery and completed JAX workload PASS on target workstation; import-only or unsupported is not completion | R-ENV-008 P0; TEST_ACCEPTANCE L6.7; user requirement |
| P11-L6-06 | Real workstation | .ipynb create/import/edit/save | BLOCKED | P11-L6-03 | Notebook workflows PASS | TEST_ACCEPTANCE L6.8 |
| P11-L6-07 | Real workstation | Persistent Session state | BLOCKED | P11-L6-06 | State survives cells | TEST_ACCEPTANCE L6.9 |
| P11-L6-08 | Real workstation | Two Sessions in parallel | BLOCKED | P11-L6-07 | Actual concurrency PASS | TEST_ACCEPTANCE L6.10 |
| P11-L6-09 | Real workstation | GPU/VRAM dashboard validation | BLOCKED | P11-L6-02 | Reasonably agrees with NVIDIA tooling | TEST_ACCEPTANCE L6.11 |
| P11-L6-10 | Real workstation | Worker crash evidence | BLOCKED | P11-L6-07 | Crash evidence displayed and isolation preserved | TEST_ACCEPTANCE L6.12 |
| P11-L6-11 | Real workstation | Human + MCP same-Session proof | BLOCKED | P11-L6-07 | secret_from_master=123456789 returned with no second worker | TEST_ACCEPTANCE L6.13 |
| P11-L6-12 | Real workstation | Secure Tunnel remote proof | BLOCKED | P11-L6-11 | Remote same-Session access PASS | TEST_ACCEPTANCE L6.14 |
| P11-L6-13 | Real workstation | Windows sign-in/restart recovery | BLOCKED | P11-L6-02 | Expected recovery behavior PASS | TEST_ACCEPTANCE L6.15 |
| P11-L6-14 | Real workstation | Active-Session update guard | BLOCKED | P11-L6-07 | No silent active-Session termination | TEST_ACCEPTANCE L6.16 |
| P11-L6-15 | Real workstation | Diagnostic bundle redaction | BLOCKED | P11-L6-12 | No protected secret leakage | TEST_ACCEPTANCE L6.17 |
| P11-L6-16 | Real workstation | Stop/restart/repair paths | BLOCKED | P11-L6-02 | Operational recovery paths PASS | TEST_ACCEPTANCE L6.18 |
| P11-L6-17 | Real workstation | Explicit v0.1 destructive reset on disposable legacy machine | BLOCKED | P11-L6-01 | Legacy reset PASS with explicit destructive boundary | TEST_ACCEPTANCE L6.19 |
| P11-SEC | Release | Security acceptance corpus | BLOCKED | P11-L6-01..17 | workload/drive/interop/secret/socket/import/MCP/diagnostic assertions PASS | TEST_ACCEPTANCE §13 |
| P11-PERF | Release | Establish and record performance baselines/targets | BLOCKED | P11-L6-01..17 | required startup/sync/session/latency/memory/metrics/notebook benchmarks recorded | TEST_ACCEPTANCE §14 |
| P11-BLOCKERS | Release | Verify all release blockers absent | BLOCKED | P11-SEC,P11-PERF | No blocker in TEST_ACCEPTANCE §15 remains | TEST_ACCEPTANCE §15 |
| P11-VERIFY | Release | Record release verification groups | BLOCKED | P11-BLOCKERS | rust-core/python-worker/frontend/protocol/notebook/mcp/host/installer evidence recorded | TEST_ACCEPTANCE §16 |
| P11-GATE | Gate | Close Phase 11 / #35 and v1 release acceptance | BLOCKED | P11-VERIFY | All mandatory L0-L6/release evidence PASS | #35 closes |

## Evidence recording rule

When changing a row to `DONE`, replace generic notes such as `implemented baseline` or `Phase 4` with concrete evidence where practical: commit SHA, test command + PASS result, smoke-test record, fixture name, or real-machine acceptance record. For a gate row, all dependencies must already be `DONE`; the gate itself records the final acceptance evidence.

## Change-control rule

If implementation or verification discovers a required work item not represented here, add it under the owning Phase before or in the same commit as the fix. Do not hide new work inside a broad row. If a specification changes behavior, update both the authoritative specification and the affected WBS rows.
