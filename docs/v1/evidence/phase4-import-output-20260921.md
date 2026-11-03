# Phase 4 import, output and completion evidence — 2026-09-21

This batch adds bounded import transactions, worker-result conversion and
revisioned output application to the document domain service. It also adds a
reliable per-operation completion receipt to the existing Session Supervisor.
It does not yet implement the public authoritative execute-cell coordinator.

## Source and environment

Same Windows/Ubuntu development environments as the previous Phase 4 document
evidence. After verification, 135 source/test/support files outside `docs/`
matched the Windows working tree and native Linux snapshot byte-for-byte.
[Hashes](phase4-import-output-20260921.sources.json) bind tested code; documentation
and ledger edits are deliberately excluded from this manifest.

## Verification

| Check | Result |
|---|---|
| Windows Rust workspace tests | 39 PASS |
| Linux Rust workspace tests | 128 PASS, no ignored or failed tests |
| Windows/Linux clippy, all targets, warnings denied | PASS |
| Rust formatting | PASS |
| Existing real-worker Supervisor smoke, extended with captured output | PASS |
| Captured real IPython stdout, HTML and expression value 124 | PASS |
| Captured real Python ValueError remains a normal failed operation | PASS |
| Simulated slow event subscriber / 2,048 outputs / complete notebook save | PASS |
| Worker crash resolves captured completion as failure | PASS |
| Wrong-Project ensure/restart rejection; original Session survives | PASS |

Linux total: domain 2, MCP scaffolding 3, protocol 4, Runtime unit 82, notebook
model 7, document integration 6 and Session process 24. This is 13 added tests
over the previous 115-test Linux workspace result. Windows excludes Linux
storage and Session tests; its three new output-conversion tests also pass.

The 2,048-output test uses a real OS subprocess/private socket with a fake worker
implementation. It deliberately leaves a broadcast listener unread, proves that
listener reports lag, then obtains every output through the independent receipt,
applies them via Notebook Document Service and rereads all 2,048 persisted
nbformat stream outputs. It does not claim to be an actual Desktop or MCP client.

The extended real-worker smoke uses the production uv/IPython launch path and
converts actual stream, rich HTML, expression and exception frames. Existing
persistent-state, FIFO, concurrency, interrupt, restart and crash/survivor checks
still run. This smoke does not implement public notebook-ID/cell-ID admission.

## Negative and boundedness proofs

- Ordered chunks stay unpublished until finish; preserved document content then
  appears through the ordinary no-overwrite atomic document installation.
- Empty, oversized, duplicate, out-of-order and total-length-overrun chunks abort
  only their transaction. Incomplete finish, hash mismatch and malformed JSON
  leave no named partial file.
- Wrong-Project cancellation does not cancel the owner's transaction. Expiry,
  explicit cancellation, maximum active imports, declared staging byte budget,
  declared size and hexadecimal digest validation are covered.
- Collisions created during upload are not overwritten. Traversal and a parent
  directory replaced by a symlink during upload cannot write outside the Project.
- Conversion rejects missing lifecycle/terminal frames, wrong operation IDs and
  output sequence gaps. Stream, display, expression and error shapes validate;
  truncation metadata remains explicit.
- Completion capture byte/event budget overflow discards partial frames and
  returns an error rather than silently producing a shorter saved notebook.
- Session ensure/restart checks ownership before returning or stopping a worker.
  A test proves the rightful owner can still execute after rejected calls.

## Failures corrected

Adding the Session ownership error initially exposed an exhaustive match missing
that new variant in failure classification. It was assigned RuntimeControl and
the Linux build, lint and workspace tests were rerun successfully. The existing
benign `/bin/kill` no-such-process diagnostic appears during the deliberate crash
smoke; correlated crash/survivor assertions and the complete smoke passed.

## Remaining gate work

P4-08 must join authoritative document/cell/revision admission, primary Session
execution, client-independent completion processing and serialized persistence.
Edits or reloads while an operation is queued need source-identity checks; ordinary
output revisions must not cause false source conflicts. P4-13 must connect
close/flush and explicit reconciliation without destroying Session memory.

Public RPC/base64 adapters, actual Windows Host streaming and actual Desktop/MCP
clients are not established by these byte-oriented component methods. The whole
Phase 4 gate and overall Desktop/Codex/ChatGPT/JAX goal remain open.

Outputs: [Linux tests](phase4-import-output-20260921.linux-output.txt),
[Windows tests](phase4-import-output-20260921.windows-output.txt),
[real worker](phase4-import-output-20260921.real-worker.txt).
