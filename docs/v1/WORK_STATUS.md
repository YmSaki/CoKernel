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

- Total tracked work items: **202**
- DONE: **43**
- READY_VERIFY: **22**
- IN_PROGRESS: **0**
- TODO: **0**
- BLOCKED: **137**
- Ledger DONE ratio (`DONE / total`, equal-weight rows, not effort or product completeness): **21.3%**
- Phase gates completed: **3 / 12 phases = 25.0%** (Phase 0, 1, 2; inherited closure records)
- Active Phase: **Phase 3 / #29**
- Phase 3: **17 / 39 implementation rows DONE, 22 READY_VERIFY; worker pytest + protocol fixtures DONE; 13 Rust/WSL/Windows verification/acceptance/gate rows BLOCKED**.

The worker-side implementation slice #39 is closed. The remaining Phase 3 critical path requires Runtime/Supervisor verification on a Rust-equipped Linux/WSL checkout and, for Windows-side checks, a Windows/WSL target. Runtime-side inspection revalidation discovered during this phase is tracked separately as `P3-INSP-05` rather than hidden inside the already-complete worker inspection rows. The original 24 DONE rows are inherited from #27/#28 closure records.

## Execution order

1. `P3-V-01` .. `P3-V-04` — run cargo fmt/check/clippy/test on a Rust-equipped Linux/WSL checkout; this also verifies `P3-INSP-05` and the other READY_VERIFY Runtime/Supervisor rows.
2. `P3-V-07` .. `P3-V-10` — run canonical `check.sh`, applicable `check.ps1`, and real Runtime Session Supervisor/inspection smokes. `check-worker.sh` includes WBS/fixture checks and a source-tree AF_UNIX smoke before its mandatory uv path.
3. `P3-A-01` .. `P3-A-04` — persistent-state, A/B concurrency, crash containment and hostile-inspection acceptance.
4. A failed test reopens its owning implementation row; fix it and rerun the affected tests in the same batch. No speculative re-audit loop and no gate bypass.
5. `P3-GATE` — close #29 and #40 only after all acceptance evidence passes; then proceed to Phase 4 in numeric WBS order.

Evidence: [E-WORKER-FULL](evidence/phase3-worker-full-e7b208f.md), [E-BATCH](evidence/phase3-worker-components-946d479.md), [E-FIX](evidence/phase3-protocol-fixtures-9a22667.md), [E-RUNTIME-INSP](evidence/phase3-runtime-inspection-51e50e5.md).

## Phase summary

| Phase | Issue | Items | DONE | READY_VERIFY | IN_PROGRESS | TODO | BLOCKED | State |
|---|---:|---:|---:|---:|---:|---:|---:|---|
| 0 | #27 | 8 | 8 | 0 | 0 | 0 | 0 | DONE |
| 1-2 | #28 | 16 | 16 | 0 | 0 | 0 | 0 | DONE |
| 3 | #29 | 54 | 19 | 22 | 0 | 0 | 13 | ACTIVE / ENV-BLOCKED |
| 4 | #30 | 14 | 0 | 0 | 0 | 0 | 14 | BLOCKED |
| 5 | #31 | 13 | 0 | 0 | 0 | 0 | 13 | BLOCKED |
| 6 | #32 | 11 | 0 | 0 | 0 | 0 | 11 | BLOCKED |
| 7 | #32 | 9 | 0 | 0 | 0 | 0 | 9 | BLOCKED |
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
| P3-INSP-05 | Inspection / Runtime boundary | Revalidate worker inspection results fail-closed before exposing callers | READY_VERIFY | P3-INSP-01..04 | Runtime rejects malformed, duplicate, oversized or internally inconsistent list/get results and preserves explicit Python None semantics | 210559b + 616ccaad + 5bd0245; 8 Rust tests + WSL smoke assertions added; cargo Gate pending |
| P3-SUP-01 | Supervisor | Own one worker process per Session | READY_VERIFY | P3-WRK-01 | Each Session has independent worker/process ownership | implemented baseline; Rust/WSL Gate pending |
| P3-SUP-02 | Supervisor | Private listener/socket + ready handshake | READY_VERIFY | P3-SUP-01 | Runtime establishes and validates worker readiness | implemented baseline; Rust/WSL Gate pending |
| P3-SUP-03 | Supervisor | Per-Session FIFO single-flight queue shared by callers | READY_VERIFY | P3-SUP-02 | Human/AI operations serialize in accepted order | implemented baseline; Rust/WSL Gate pending |
| P3-SUP-04 | Supervisor | Interrupt active execution | READY_VERIFY | P3-SUP-03 | Interrupt terminates active operation without corrupting Session | implemented baseline; Rust/WSL Gate pending |
| P3-SUP-05 | Supervisor | Graceful stop | READY_VERIFY | P3-SUP-02 | Session stops cleanly and reports state | implemented baseline; Rust/WSL Gate pending |
| P3-SUP-06 | Supervisor | Restart Session | READY_VERIFY | P3-SUP-05 | Worker restarts with explicit Session semantics | implemented baseline; Rust/WSL Gate pending |
| P3-SUP-07 | Supervisor | Heartbeat timeout/liveness semantics | READY_VERIFY | P3-WRK-02,P3-SUP-02 | Lost/stale worker is detected deterministically | actor heartbeat_timeout path present; canonical Gate pending |
| P3-SUP-08 | Supervisor | Crash containment | READY_VERIFY | P3-SUP-01 | Worker crash does not kill Runtime or other Sessions | implemented baseline; Rust/WSL Gate pending |
| P3-SUP-09 | Supervisor | FailureRecord capture | READY_VERIFY | P3-SUP-08 | Crash records exit/failure evidence and last operation | implemented baseline; Rust/WSL Gate pending |
| P3-SUP-10 | Supervisor | Environment generation/stale marking | READY_VERIFY | P12-13,P3-SUP-01 | Existing Session stays alive but becomes stale after env mutation | implemented baseline; Rust/WSL Gate pending |
| P3-SUP-11 | Supervisor | Concurrent independent Sessions | READY_VERIFY | P3-SUP-01 | A/B overlap and namespaces remain isolated | implemented baseline; Rust/WSL Gate pending |
| P3-LC-01 | Protocol invariants | Bind worker frames to active transaction | READY_VERIFY | P3-SUP-03 | Stale/wrong transaction frames fail closed | 4eeb763 lineage; Rust tests pending |
| P3-LC-02 | Protocol invariants | Require single execution_started before outputs | READY_VERIFY | P3-LC-01 | Duplicate/missing/out-of-order start rejected | acefea7 lineage; Rust tests pending |
| P3-LC-03 | Protocol invariants | Allow outputs only between start and finish | READY_VERIFY | P3-LC-02 | Output before start/after finish rejected | acefea7 lineage; Rust tests pending |
| P3-LC-04 | Protocol invariants | Require contiguous output sequence 1..N | READY_VERIFY | P3-LC-02 | Sequence gaps/duplicates rejected | acefea7 lineage; Rust tests pending |
| P3-LC-05 | Protocol invariants | Validate execution_finished.output_count | READY_VERIFY | P3-LC-04 | Count equals accepted outputs | acefea7 lineage; Rust tests pending |
| P3-LC-06 | Protocol invariants | Require final response after execution_finished | READY_VERIFY | P3-LC-05 | Early/missing response rejected | acefea7 lineage; Rust tests pending |
| P3-LC-07 | Protocol invariants | Align status/execution_count between finish and response | READY_VERIFY | P3-LC-06 | Terminal response agrees with lifecycle | acefea7 lineage; Rust tests pending |
| P3-LC-08 | Protocol invariants | Preflight oversized execute requests | READY_VERIFY | P3-SUP-03 | Oversized caller request rejected without crashing healthy Session | ecdcc65 lineage; Rust tests pending |
| P3-LC-09 | Protocol invariants | Bind final result.operation_id exactly to active Runtime operation | READY_VERIFY | P3-LC-06 | Mismatch is rejected fail-closed with regression test | de52f4b + c68db10; Rust tests pending |
| P3-LC-10 | Protocol invariants | Validate truncation accounting self-consistency and final-response agreement | READY_VERIFY | P3-EXE-07,P3-LC-05 | Flags/omitted bytes/reasons are internally consistent and identical at finish/response | c68db10; Rust tests pending |
| P3-V-01 | Verification | cargo fmt --check | BLOCKED | P3-WRK-01..P3-LC-10 | exit 0 on Rust-equipped checkout | cargo/rustfmt unavailable in current executor |
| P3-V-02 | Verification | cargo check --workspace | BLOCKED | P3-WRK-01..P3-LC-10 | exit 0 | cargo/rustc unavailable in current executor |
| P3-V-03 | Verification | cargo clippy --workspace --all-targets -- -D warnings | BLOCKED | P3-WRK-01..P3-LC-10 | exit 0 with no warnings | Rust toolchain unavailable in current executor |
| P3-V-04 | Verification | cargo test --workspace | BLOCKED | P3-WRK-01..P3-LC-10 | all tests pass | Rust toolchain unavailable in current executor |
| P3-V-05 | Verification | worker pytest | DONE | P3-WRK-01..P3-LC-10 | all worker tests pass | E-WORKER-FULL: exact-source worker/tests 137/137 PASS; failures/errors/skips 0 |
| P3-V-06 | Verification | protocol fixture validation | DONE | P3-WRK-01..P3-LC-10 | all fixtures pass | E-FIX: 5/5 PASS |
| P3-V-07 | Verification | scripts/v1/check.sh | BLOCKED | P3-V-01..06 | canonical Linux/WSL local gate exits 0 | worker source preflight integrated; uv dependency sync + Rust Gate unavailable here |
| P3-V-08 | Verification | scripts/v1/check.ps1 applicable checks | BLOCKED | P3-V-01..06 | canonical Windows-side checks exit 0 | Windows gate now mirrors WBS/script/fixture/source diagnostics and requires mandatory WSL check.sh; no Windows target here |
| P3-V-09 | Verification | Supervisor real-process/socket smoke | BLOCKED | P3-V-07 | start/handshake/queue/interrupt/restart/stop/crash paths pass | Rust/WSL Gate pending |
| P3-V-10 | Verification | Inspection hostile-object smoke | BLOCKED | P3-V-07 | safe inspection passes against real worker | Runtime validator + Python None assertion added; Rust/WSL Gate pending |
| P3-A-01 | Acceptance | Persistent state proof: x=123 then x+1 == 124 | BLOCKED | P3-V-09 | returns 124 in same Session | Runtime Supervisor proof pending; worker proof is not substitute |
| P3-A-02 | Acceptance | Parallel A/B Session proof | BLOCKED | P3-V-09 | actual overlap + isolated namespace/PID | Runtime Supervisor proof pending |
| P3-A-03 | Acceptance | Crash containment proof | BLOCKED | P3-V-09 | kill A; B survives; A has FailureRecord | Runtime Supervisor proof pending |
| P3-A-04 | Acceptance | Hostile inspection acceptance | BLOCKED | P3-V-10 | hostile object corpus does not execute user hooks | Runtime real-worker proof pending |
| P3-GATE | Gate | Close Phase 3 / #29 | BLOCKED | P3-A-01..04 | All Phase 3 deliverables and canonical gate evidence PASS | #29 remains open; no gate waiver |

## Phase 4 — Notebook Document Service (#30)

| ID | Work package | Work item | State | Depends on | Done when | Evidence / note |
|---|---|---|---|---|---|---|
| P4-01 | Notebook model | Implement nbformat v4 Rust model | BLOCKED | P3-GATE | Supported notebooks parse/serialize structurally valid | Phase 4 |
| P4-02 | Notebook model | Preserve unknown notebook/cell metadata | BLOCKED | P4-01 | Round-trip preserves unknown metadata | Phase 4 |
| P4-03 | Document service | Create/open/list notebooks | BLOCKED | P4-01 | Project notebook CRUD read paths work | Phase 4 |
| P4-04 | Document service | Revisioned edits and conflict detection | BLOCKED | P4-03 | Stale revision is rejected without overwrite | Phase 4 |
| P4-05 | Persistence | Atomic save | BLOCKED | P4-03 | Save is atomic and failure-safe | Phase 4 |
| P4-06 | Persistence | External-change detection | BLOCKED | P4-03 | External modification causes explicit conflict | Phase 4 |
| P4-07 | Execution integration | Convert worker events to nbformat outputs | BLOCKED | P3-GATE,P4-01 | Streams/errors/rich MIME persist validly | Phase 4 |
| P4-08 | Execution integration | Execute cell by authoritative notebook/cell ID | BLOCKED | P4-03,P4-07 | Correct cell executes in Project Session | Phase 4 |
| P4-09 | Import | Windows->WSL notebook import transaction | BLOCKED | P4-03 | Chunk/import transaction handles bytes safely | Phase 4 |
| P4-10 | Import | Validate import hash/size/path/collision | BLOCKED | P4-09 | Mismatch/path escape/collision rejected | Phase 4 |
| P4-11 | Compatibility | Build notebook fixture suite | BLOCKED | P4-01 | Required fixtures cover streams/errors/HTML/PNG/SVG/attachments/metadata/large/malformed | TEST_ACCEPTANCE §11 |
| P4-12 | Verification | Notebook unit/component/fixture tests | BLOCKED | P4-01..11 | All notebook tests pass | Phase 4 |
| P4-A-01 | Acceptance | Import/execute/persist standard .ipynb proof | BLOCKED | P4-12 | Imported notebook executes, persists outputs, remains valid | Phase 4 gate |
| P4-GATE | Gate | Close Phase 4 / #30 | BLOCKED | P4-A-01 | All Phase 4 evidence PASS | #30 closes |

## Phase 5 — Windows Host + WSL Runtime bridge (#31)

| ID | Work package | Work item | State | Depends on | Done when | Evidence / note |
|---|---|---|---|---|---|---|
| P5-01 | Host | Create cokernel-host Windows binary/crate | BLOCKED | P4-GATE | Host builds and starts | Phase 5 |
| P5-02 | Bridge | Own long-lived wsl.exe runtime bridge | BLOCKED | P5-01 | Host owns persistent bridge process | Phase 5 |
| P5-03 | Bridge | Protocol handshake/version mismatch handling | BLOCKED | P5-02 | Compatible handshake succeeds; mismatch fails clearly | Phase 5 |
| P5-04 | Lifetime | Persist desired RUNNING/STOPPED state | BLOCKED | P5-02 | Host converges runtime to desired state | Phase 5 |
| P5-05 | Recovery | Reconnect/backoff/circuit breaker | BLOCKED | P5-02 | Bridge failures recover boundedly | Phase 5 |
| P5-06 | IPC | Current-user Named Pipe RPC/events | BLOCKED | P5-01 | Authorized local clients can RPC/subscribe | Phase 5 |
| P5-07 | IPC | Event forwarding and request correlation | BLOCKED | P5-03,P5-06 | Runtime events correlate correctly to clients | Phase 5 |
| P5-08 | Autostart | Install/user auto-start behavior | BLOCKED | P5-01 | Host can run independently of Desktop | Phase 5 |
| P5-09 | Metrics | Aggregate Windows + WSL metrics | BLOCKED | P5-02 | CPU/RAM/WSL RAM/GPU/VRAM/storage dashboard | Phase 5 |
| P5-10 | Import bridge | Stream Windows notebook bytes to Runtime | BLOCKED | P4-GATE,P5-06 | Notebook import works without unsafe path assumptions | Phase 5 |
| P5-11 | Verification | Host fake-runtime component tests | BLOCKED | P5-01..10 | desired state/reconnect/ACL/events/secrets/UI disconnect tests pass | TEST_ACCEPTANCE L2 Host |
| P5-A-01 | Acceptance | UI-independent Host/Runtime lifetime proof | BLOCKED | P5-11 | Host keeps Runtime/Sessions alive across client reconnects | Phase 5 gate |
| P5-GATE | Gate | Close Phase 5 / #31 | BLOCKED | P5-A-01 | All Phase 5 evidence PASS | #31 closes |

## Phase 6 — CoKernel-native MCP (#32)

| ID | Work package | Work item | State | Depends on | Done when | Evidence / note |
|---|---|---|---|---|---|---|
| P6-01 | MCP service | Create cokernel-mcp service | BLOCKED | P5-GATE | Service starts locally | Phase 6 |
| P6-02 | Transport | Implement Streamable HTTP | BLOCKED | P6-01 | MCP transport interoperates with client | Phase 6 |
| P6-03 | Security | Require local bearer auth | BLOCKED | P6-01 | Unauthenticated calls rejected | Phase 6 |
| P6-04 | Runtime client | Implement Runtime Unix API client | BLOCKED | P3-GATE,P6-01 | MCP calls Runtime domain API | Phase 6 |
| P6-05 | Tool surface | Project/Notebook/Session/Variable/Resource tools | BLOCKED | P6-04 | Minimum v1 domain tool set exists | Phase 6 |
| P6-06 | Tool contracts | Exact annotations/schema snapshots | BLOCKED | P6-05 | Annotations and schemas match intended capabilities | Phase 6 |
| P6-07 | Bounds | Bound MCP responses and validate inputs | BLOCKED | P6-05 | Huge/invalid requests cannot escape limits | Phase 6 |
| P6-08 | Queue integration | Human + MCP shared Session FIFO semantics | BLOCKED | P3-GATE,P6-04 | MCP and Human serialize through same Session queue | Phase 6 |
| P6-09 | Verification | MCP contract/security suite | BLOCKED | P6-01..08 | bearer/tools/inspection/revision/bounds tests pass | TEST_ACCEPTANCE L5 MCP local |
| P6-A-01 | Acceptance | Local same-Session secret_from_master proof | BLOCKED | P6-09 | MCP get_variable returns 123456789 with no second worker | Canonical proof |
| P6-GATE | Gate | Close Phase 6 portion of #32 | BLOCKED | P6-A-01 | Local native MCP gate PASS | Phase 6 |

## Phase 7 — Secure Tunnel integration (#32)

| ID | Work package | Work item | State | Depends on | Done when | Evidence / note |
|---|---|---|---|---|---|---|
| P7-01 | Secrets | Windows Secret Store abstraction | BLOCKED | P6-GATE | Credentials stored outside worker/runtime-visible channels | Phase 7 |
| P7-02 | Secrets | Controlled credential handoff | BLOCKED | P7-01 | Only tunnel/MCP components receive required secret | Phase 7 |
| P7-03 | Tunnel | Tunnel Supervisor | BLOCKED | P6-GATE | Tunnel process lifecycle is supervised | Phase 7 |
| P7-04 | Tunnel | MCP-before-tunnel startup/readiness ordering | BLOCKED | P7-03 | Tunnel starts only after MCP ready | Phase 7 |
| P7-05 | Tunnel | Reconnect/backoff and remote-access status | BLOCKED | P7-03 | Tunnel recovers and status is observable | Phase 7 |
| P7-06 | Security | Credential isolation/redaction tests | BLOCKED | P7-01..05 | No tunnel/MCP secret reaches worker/log/diagnostics | Phase 7 |
| P7-A-01 | Acceptance | Remote same-Session proof | BLOCKED | P7-06 | Remote MCP reads Human-created value from same Session | Phase 7 gate |
| P7-A-02 | Acceptance | Tunnel-disconnect local-survival proof | BLOCKED | P7-06 | Disconnect/restart leaves local execution healthy | Phase 7 gate |
| P7-GATE | Gate | Close Phase 7 / #32 | BLOCKED | P7-A-01..02 | All MCP+tunnel evidence PASS | #32 closes |

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
| P11-L6-05 | Real workstation | JAX GPU smoke when supported | BLOCKED | P11-L6-03 | JAX GPU workload PASS or matrix marks unsupported | TEST_ACCEPTANCE L6.7 |
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
