# Phase 5 Project ownership and P5-07 closure — 2026-09-22

Base: `f35ca99711a8d6c102ef9e47b857e0e377f87cbb` plus the working tree.
The reopened **P5-07** common admission/completion refinement is now locally DONE.
This does not close Phase 5, any GitHub issue, or the Desktop/MCP/JAX GPU goal.
No commit or PR was created.

## Project storage implementation

One Project owner serializes filesystem work off Runtime admission. A read-only
projection supplies Project identity and conservative environment state without
waiting on storage. Mutable managers are not cloned; the production authority
rejects accidental synchronous storage dispatch. The synchronous CLI/component
harness remains available separately.

Create/register/package preparation, canonical path resolution, write-ahead
uncertainty, status probes, post-uv reconciliation and registry removal use this
owner. Preparation returns through the common bounded request-work collection
with typed completion. Canonical path and Project reservations prevent aliases
or concurrent lifecycle work from starting a second mutation. The four-operation
environment limit includes preparation, and the storage queue holds at most
sixteen waiting jobs plus one running job.

uv starts only after write-ahead preparation succeeds. Its operation remains
RUNNING/reserved until owned reconciliation finishes. The owner publishes the
updated projection before replying, including uncertain state retained after
failure. Stale notification and terminal events follow reconciliation.

Removal checks live Sessions/pending document work and reserves identity through
the registry write. On failure it restores the in-memory record for retry and
never deletes files. A post-rename failure still leaves durability uncertain;
the error is not a successful removal acknowledgement.

EOF closes environment launch before draining accepted preparation. New durable
identities remain available for sync-based recovery, without a late uv launch.
Existing uv processes retain cancellation/reaping/reconciliation ownership.
Dropping a request ticket cannot cancel accepted storage work. Queue capacity and
owner failures are explicit; a failed owner makes its projection uncertain.

## Added proofs and compatibility correction

Six tests demonstrate:

1. A storage barrier before package preparation leaves an actual component
   worker's status/interrupt responsive through RuntimeHandle, does not launch
   uv early, and rejects a conflicting mutation.
2. Holding reconciliation after uv starts leaves the operation RUNNING, the
   Project reserved, and uncertainty observable both in memory and in a freshly
   loaded registry. Release permits successful completion.
3. Pending removal rejects new document/startup work. Injected registry write
   failure retains the Project; repaired storage permits retry without deleting
   its notebook or creating rejected files.
4. Owner shutdown during held creation drains preparation but launches no uv;
   a freshly loaded registry retains the incomplete identity for later sync.
5. Queue overflow occurs before mutation; dropped replies do not erase accepted
   work. An injected owner panic reports failure and exposes only uncertain state.
6. Canonical and symlink aliases compete for one Project reservation. Exactly
   one environment operation is accepted and the original identity is preserved.

An existing package test initially found that create-on-existing returned
CK-UV-BUSY during a mutation instead of the established CK-PROJ-EXISTS. Admission
now checks the existing identity first, preserving the old result. The existing
test passes unchanged. All previous package cancellation, panic, partial-change,
registry-commit-failure and retained-history tests pass.

## Final verification

| Check | Result |
|---|---|
| Windows check.ps1 including actual WSL check.sh | PASS, exit 0 |
| Native Rust workspace | 76 unique tests; fmt/check/clippy PASS |
| Linux Rust workspace | 197 unique tests; fmt/check/clippy PASS |
| Native worker | 163 PASS; eight Unix-only skips |
| Linux worker | 171 PASS |
| Real uv/IPython Session/notebook/Project/package/shared-environment smokes | PASS |
| Windows/WSL source identity before/after gate | 127 files match |
| Actual Windows Named Pipe/Host/WSL proof | PASS, exit 0 |
| Independent nbformat/output/attachment/shutdown-edit/worker-exit validation | PASS, exit 0 |

Nested credential subprocess tests are counted once. Documentation, evidence and
ledger closure followed source verification; runtime sources did not change.
The gate's ledger check therefore records the pre-closure 103-DONE count; the
final ledger is independently checked after changing P5-07 to DONE.

Actual Host root:
`/home/ymsaki/.cache/cokernel-v1-dev/host-proof-55aeeb9d91f9`.
Native state: `target/dev-tools/host-project-owner-state`.
The proof checks shared primary, retained accepted execution after disconnect,
control during pending inspection, Project/package completion events, Windows
chunked import, output persistence, shutdown flushing and reconnect behavior.
Independent validation confirms persisted output `124` and worker exit.

## P5-07 completion audit

| Required boundary | Authoritative evidence |
|---|---|
| Transport-independent bounded admission, unique internal correlation, caller lifetime | Endpoint tests and endpoint evidence, rerun in this full gate |
| Shared primary and Human/AI FIFO; distinct Notebook concurrency | Session/real uv-IPython tests, including same-Project package origin and separate namespaces |
| Startup/inspection completion outside admission, lifecycle exclusion and cleanup | Deferred-request tests and actual-worker Session RPC smoke |
| Owned document storage and ordered execution preparation | Document barrier/cancellation/shutdown tests in the full gate |
| Control cannot wait for a saturated Session queue | Control coalescing/first-poll burst/surviving-Notebook tests |
| Project storage lifetime and reservations | Six new owner tests plus existing package/recovery tests and real smokes |
| Correlated events and local Host lifetime across clients | Actual Windows Named Pipe/Host/WSL proof and independent saved-file/exit checks |

Explicit shutdown still joins owned cleanup under documented budgets. This is
not an OS/hardware I/O-hang guarantee. Session status, ordinary Session control
and conflicting-domain admission do not inherit document/Project storage waits.

P5-08 autostart and P5-09 resource metrics are next, followed by Phase 5 acceptance.
Actual Desktop, Codex/Web ChatGPT MCP, managed-WSL security and JAX GPU acceptance
remain open. Rust caller origins and development Ubuntu do not substitute for
those real-client/workstation requirements.

Artifacts: `phase5-project-owner-20260922.gate.txt`,
`phase5-project-owner-20260922.real.txt`, and
`phase5-project-owner-20260922.sources.json`.
