# Phase 5 owned document work evidence — 2026-09-22

Base: `f35ca99711a8d6c102ef9e47b857e0e377f87cbb` plus the working tree.
This is a verified implementation slice within **P5-07**, not closure of P5-07,
P5-GATE or the actual Desktop/MCP/JAX GPU goal. No commit or PR was created.

## Ownership and ordering

Notebook filesystem work now belongs to a bounded Document Service FIFO owner.
It joins each blocking job before starting the next. Public admission reserves
capacity without waiting for disk; the queue holds at most 32 waiting jobs and
one running job. Accepted execution persistence remains owned even when callers
disconnect. Workers still return structured results and never persist notebooks.

Document replies, snapshots, Session startup/inspection and execution preparation
use one bounded request-work collection. The earlier session-specific scheduler
was renamed and generalized. Snapshot `request_work` is the common summary;
`session_requests` remains a compatibility alias.

Notebook execution preparation reserves its FIFO turn during admission. Later
futures cannot overtake earlier preparation, including cancellation of a middle
request. Turns end after enqueueing, so they do not serialize different notebooks'
worker execution. Restart/ensure and Project mutation reject conflicting pending
preparation; stop cancels and joins it before stopping the Session.

A small NotebookId-to-ProjectId index rejects wrong ownership before reserving
startup, even when storage is stalled. Full document validation still precedes
worker launch. Project removal rejects pending document work.

Shutdown drains accepted document writes. A five-second drain/flush wait returns
CK-RUN-SHUTDOWN when storage has not recovered; the Runtime remains available.
The total explicit response budget is twelve seconds, inside the Host's fifteen
seconds. Accepted file jobs retain their owner after timeout. EOF joins owned work;
this does not promise recovery from an indefinite OS/hardware I/O hang.

## Added regression proofs

- Hold Document Service behind a barrier while another Session answers status
  and interrupt. While held, reject wrong-Project startup, Project removal,
  restart and package mutation as appropriate. Stop cancels preparation, and
  releasing storage cannot execute the cancelled code. A dropped edit reply does
  not lose the accepted edit.
- Admit three execution preparations, cancel the middle and poll the last first.
  The last remains pending until the first is enqueued; execution events preserve
  Human then MCP order.
- Hold an accepted edit across shutdown. Shutdown returns a bounded refusal,
  ping still works, storage release completes the edit and final cleanup saves it.
- Fill the document queue, drop a reply, and reject the excess request before
  mutation. All accepted files are created. An injected failed job resolves as
  an error and does not prevent the next job from running.

## Verification

| Check | Result |
|---|---|
| Windows check.ps1 including actual WSL check.sh | PASS, exit 0 |
| Native Rust workspace | 76 unique tests PASS; fmt/check/clippy PASS |
| Linux Rust workspace | 187 unique tests PASS; fmt/check/clippy PASS |
| Native worker | 163 PASS; eight Unix-only skips |
| Linux worker | 171 PASS |
| Real uv/IPython Session, notebook, package and Project smokes | PASS |
| Same-Project notebooks: shared package origin, isolated variables, concurrent execution | PASS |
| Windows/WSL source identity before and after gate | 122 files match |
| Actual Windows Named Pipe/Host/WSL proof | PASS, exit 0 |
| Independent nbformat/output/attachment/shutdown-edit/worker-exit checks | PASS, exit 0 |

Nested credential-test subprocess results are counted once. Documentation and
evidence recording followed source verification without changing runtime sources.

The actual Host proof used fresh root
`/home/ymsaki/.cache/cokernel-v1-dev/host-proof-832ed57a1ebb` and native state
`target/dev-tools/host-document-owner-state`. It confirms a shared primary,
accepted execution after client disconnect, asynchronous completion events,
control during inspection, chunked Windows import, save and shutdown/reopen.
Independent validation confirms persisted output `124` and worker exit.
These callers are Rust clients, not Desktop or actual MCP clients. The Ubuntu
development distribution is not the managed-WSL security/JAX acceptance proof.

## Intermittent executable-fixture failure

An earlier gate in this batch failed one existing fake-worker test with Linux
ETXTBSY (`Text file busy`). The earlier UUID directory change did not eliminate
this failure. A focused rerun and strace run passed; the precise cause has not
been proven. The failure is retained rather than being counted as a passing gate.

The fake launcher is now initialized once before any test in that test process
can spawn it, closed for writing, and shared immutably. Its unique file is kept
beside the test binary as a build artifact. This removes per-test executable
writes concurrent with other tests' fork/exec windows. Product launch behavior,
timeouts and assertions were not weakened. The final full gate passes after this
change; this is not a claim that the original OS-level cause was established.

## Recorded artifacts and remaining work

- `phase5-document-owner-20260922.gate.txt`: final complete gate output.
- `phase5-document-owner-20260922.real.txt`: actual Host and independent checks.
- `phase5-document-owner-20260922.sources.json`: exact source SHA256 inventory.
- `phase5-document-owner-20260922.before-fixture.txt`: failed gate evidence.

P5-07 remains IN_PROGRESS. Project registry prepare/reconcile/persistence and
some Session control queue sends still wait inside admission/completion. Their
ownership, reservation and saturation behavior must be completed before this
common-service refinement closes. Autostart, resource metrics and subsequent
MCP/Tunnel/Desktop/install/full acceptance work remain in the existing ledger.
