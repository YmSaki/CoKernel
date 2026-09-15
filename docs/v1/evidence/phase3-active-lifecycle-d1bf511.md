# Phase 3 active Session lifecycle component evidence

Date: 2026-09-15

Base revision: `d1bf5112cadc7bb2ffdbe4dd234dcbfa037e67bf`

Implementation revisions:

- `59d95e1e9496c6768371acfcee68c11aad969842` — add a delayed fake-worker crash mode so a second operation can be deterministically queued before process exit.
- `c1d3e59ffe734e44e33ebbd9ed14dd818e571306` — add real-process Session Supervisor component tests for active stop/crash/restart/stale-environment semantics.

## Scope

This slice remains inside Phase 3 and does not cross `P3-GATE`. It adds five actionable component cases around existing Supervisor behavior:

1. `stop_during_execution_cancels_active_and_queued_operations`
   - active Human execution enters `EXECUTING`;
   - an MCP operation is queued behind it;
   - explicit stop requires both operations to finish `CANCELLED`, queue depth to return to zero, Session to reach `STOPPED`, and no `FailureRecord` to be emitted.
2. `crash_during_execution_fails_active_and_cancels_queued_operation`
   - fake worker heartbeats while an active operation is running, then exits with code 23;
   - active operation must finish `FAILED`;
   - queued operation must finish `CANCELLED`;
   - FailureRecord remains correlated to the active operation and Session becomes `CRASHED`.
3. `restart_during_execution_cancels_old_work_and_replaces_worker_generation`
   - restart while an active + queued operation exist cancels both old-worker operations;
   - logical Session ID and `started_at` are preserved;
   - worker generation increments;
   - replacement adopts the supplied current environment generation and can execute successfully.
4. `crashed_primary_restarts_explicitly_with_same_logical_session`
   - a crashed primary is not silently replaced;
   - explicit restart preserves logical Session identity, increments worker generation, and produces a usable replacement worker.
5. `environment_change_during_execution_becomes_stale_after_completion_and_remains_usable`
   - environment mutation during execution does not kill the current operation;
   - after completion the Session becomes `STALE_ENVIRONMENT` while retaining its original environment generation;
   - execution remains available on the stale Session and returns to `STALE_ENVIRONMENT` afterward.

These tests strengthen `P3-SUP-03`, `P3-SUP-05`, `P3-SUP-06`, `P3-SUP-08`, `P3-SUP-09`, and `P3-SUP-10`; their WBS state remains `READY_VERIFY` until the Rust/WSL gate executes.

## Source identity and diff scope

GitHub comparison from `d1bf511` to `c1d3e59` contains exactly two commits and two modified files:

- `crates/cokernel-runtime/tests/fixtures/fake_uv.py`
- `crates/cokernel-runtime/tests/session_fake_worker.rs`

Committed blobs:

- fake worker fixture: `b3c898eebd50d4b54ed0f99cc482b615a3b8ecd4`
- Rust component test: `7314e95cbd6f83454dbcb8316da014d5fd78f998`

The fake worker blob exactly matches the locally exercised file (`git hash-object /tmp/fake_uv.py` -> `b3c898eebd50d4b54ed0f99cc482b615a3b8ecd4`). The committed Rust test blob was re-read after the write; it has not been compiled in this executor.

## Non-hosted verification performed

Available environment:

- Python 3.13
- `uv`
- `git`
- no `cargo`
- no `rustc`
- no `rustfmt`
- no connected Windows/WSL execution target

Checks performed on the exact committed fake-worker source:

1. `python3 -m py_compile` — **PASS**.
2. Delayed-crash AF_UNIX subprocess smoke — **PASS**:
   - ready event PID matched the spawned process;
   - handshake succeeded;
   - `crash_after:0.15` emitted `execution_started` and heartbeat traffic;
   - socket closed on worker exit;
   - process exit status was exactly 23.
3. SIGINT/survival AF_UNIX subprocess smoke — **PASS**:
   - long execution received real SIGINT;
   - terminal execute response was `FAILED` as expected from the fake worker;
   - same worker accepted and completed a subsequent execute request;
   - cooperative shutdown exited 0.
4. Branch diff was re-read after both commits and is limited to the two files listed above.

## Not claimed / remaining gate

The added Rust component tests have **not** been compiled or executed here. This executor has no Rust toolchain, and the repository cannot be cloned through the container because external DNS for GitHub is unavailable.

Still required before Phase 3 can close:

- `cargo fmt --check`
- `cargo check --workspace`
- `cargo clippy --workspace --all-targets -- -D warnings`
- `cargo test --workspace`
- `scripts/v1/check-rust.sh`
- `scripts/v1/check.sh`
- applicable native Windows `scripts/v1/check.ps1`
- real `session_supervisor_smoke` and `session_inspection_smoke`
- `P3-A-01..04` canonical persistent-state / parallel-Session / crash-containment / hostile-inspection acceptance

No Phase 4 implementation was started. GitHub Actions / GitHub-hosted CI were not used, triggered, inspected, or monitored.
