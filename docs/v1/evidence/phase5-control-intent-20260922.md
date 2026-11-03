# Phase 5 Session control admission evidence — 2026-09-22

Base: `f35ca99711a8d6c102ef9e47b857e0e377f87cbb` plus the working tree.
This completes the control-capacity part of the reopened P5-07 refinement.
Project registry storage work remains open; P5-07 and the full delivery goal
remain IN_PROGRESS. No commit, PR or phase-gate closure was performed.

## Implemented

Session interrupt/stop/environment-stale admission no longer awaits a bounded
control queue slot. The generation owns one coalesced pending interrupt plus
sticky stop/stale flags. Repeated requests do not allocate extra jobs or sender
tasks. The actor observes stop before stale and interrupt; closed actors reject
requests. New generations receive fresh control state.

Public control methods retain their API shape, but complete on their first poll
without needing the actor to consume anything. Supervisor stale notification
still takes only the short Session-table lock and cannot wait behind a control
burst. No worker protocol or user-code FIFO changes were made. Stop completion
still includes the existing graceful/forced process cleanup; an accepted control
reply is not a declaration that the process has exited.

The existing execution-start/SIGINT guard, namespace preservation on environment
change, restart isolation and shutdown ownership remain in force. Interface and
execution specifications describe the coalescing and priority semantics.

## Regression evidence

Three control tests cover 10,000 interrupt/stale submissions with no consumer,
stop priority, repeated stale coalescing, a subsequent interrupt after consumption,
sender closure, cancelled receive waits and rejection after actor exit.

A Session integration test starts two notebook workers in one Project and long
code in the first. Without yielding to either actor, it sends 1,024 interrupts,
marks the Project's Sessions stale and sends stop. Every call must return Ready
on its first poll. The common Runtime endpoint still answers the second Session's
status; that Session executes successfully, stays the same primary, and becomes
STALE_ENVIRONMENT. The stopped Session's captured operation is cancelled and its
state reaches STOPPED. These workers use the component fake-worker protocol;
the canonical smokes below separately exercise actual uv/IPython.

The first version of this new test incorrectly allotted only three seconds to
observe stop completion, exactly the production graceful deadline. It timed out
while forced cleanup was still due. Its final observation bound is five seconds,
covering the existing three-second deadline and forced cleanup. The first-poll
admission assertions and requirement that the other Session execute before this
wait are unchanged. No product timeout or pre-existing test was relaxed.

## Full verification

| Check | Result |
|---|---|
| Windows check.ps1 including actual WSL check.sh | PASS, exit 0 |
| Native Rust | 76 unique tests; fmt/check/clippy PASS |
| Linux Rust | 191 unique tests; fmt/check/clippy PASS |
| Native worker | 163 PASS; eight Unix-only skips |
| Linux worker | 171 PASS |
| Actual uv/IPython Session/notebook/Project/package/shared-environment smokes | PASS |
| Windows/WSL source identity before/after gate | 123 files match |
| Actual Windows Named Pipe/Host/WSL proof | PASS, exit 0 |

Nested credential subprocess tests are counted once. Source hashes identify the
tested working tree; documentation/evidence edits followed verification.

Actual Host proof root:
`/home/ymsaki/.cache/cokernel-v1-dev/host-proof-2cc6d53aa63d`.
Windows state: `target/dev-tools/host-control-intent-state`.
It checks shared primary, retained accepted execution after disconnect, control
during pending inspection, event-driven completion, chunked import, saved outputs
and shutdown flushing. Rust clients do not constitute actual Desktop/MCP proof.
Development Ubuntu does not close managed-WSL security or JAX GPU acceptance.

Artifacts:

- `phase5-control-intent-20260922.gate.txt`: full canonical output.
- `phase5-control-intent-20260922.real.txt`: Host proof and independent saved-file/exit checks.
- `phase5-control-intent-20260922.sources.json`: exact source SHA256 inventory.

Next work is the owned Project registry lifecycle described in DELIVERY_PLAN:
prepare, durable uncertainty before uv, reconciliation, status probes and removal,
with reservations and failed-write recovery. It remains unimplemented in this
slice and must pass its own barrier/real-process proofs before P5-07 closes.
