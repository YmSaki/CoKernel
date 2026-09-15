# Phase 3 fake-worker lifecycle component evidence

Date: 2026-09-15

Base revision: `0f1110859852a52e542ac4627f9a80bcfb642af0`

Implementation revisions:

- `2a49ba12c56d7034031a3f3b39a11eb5cfee9171` — make the fake worker emit heartbeats while cooperatively accepting Session shutdown.
- `f7fc1ed15bbeb16f603599909db4aa1a79d0f53f` — add Session Supervisor component tests for graceful stop, explicit restart, and environment-stale isolation.

## Scope

This slice adds independent Phase 3 component coverage without crossing the Phase 3 gate or introducing Desktop/MCP/Jupyter Server execution paths.

The new Rust integration tests cover:

- `P3-SUP-05`: cooperative `stop()` reaches `STOPPED`, clears queue depth, and does not emit a `FailureRecord`;
- `P3-SUP-06`: explicit `restart_primary()` preserves the logical Session ID and `started_at`, increments `worker_generation`, and leaves the replacement worker `IDLE`;
- `P3-SUP-10`: restart adopts the supplied current Project `environment_generation`; `mark_project_environment_stale()` marks only matching live Project Sessions, leaves an unrelated Session `IDLE`, and `ensure_primary()` does not silently replace the stale primary.

The fake worker now has a cooperative heartbeat/control loop for `healthy` and `lifecycle*` modes. It continues to preserve the existing hostile modes used for heartbeat timeout, invalid handshake, ready-PID mismatch, and idle protocol-violation tests.

## Source identity

The exact locally checked files match the committed Git blob IDs:

- `crates/cokernel-runtime/tests/fixtures/fake_uv.py`: `c29494dfd26a61dd9825e05922042e48907082a9`
- `crates/cokernel-runtime/tests/session_fake_worker.rs`: `4332b9d6c0edafbce0505aaf497cab702c629cbc`

The branch comparison from the base revision to `f7fc1ed` contains exactly two modified files: the fake worker fixture and the Session Supervisor integration test.

## Non-hosted verification performed

Environment available in this executor:

- Python `3.13.5`
- pytest `9.0.2`
- no `cargo`
- no `rustc`
- no `rustfmt`
- no connected Windows/WSL CoKernel execution target

Checks performed:

1. `python3 -m py_compile crates/cokernel-runtime/tests/fixtures/fake_uv.py` — **PASS** on the exact fixture content later committed as blob `c29494d`.
2. Standalone AF_UNIX subprocess smoke against that exact fixture — **PASS**:
   - Supervisor-side listener accepted the fake worker;
   - `ready` carried the actual worker PID;
   - handshake succeeded and advertised the required shutdown capability;
   - at least one heartbeat arrived;
   - a `shutdown` request caused cooperative process exit with status 0.
3. Rust test-source delimiter/structure sanity — **PASS**. This is only a textual sanity check and is not a Rust compile/test result.
4. Local `git hash-object` for both tested files matched the content SHA returned by GitHub after each commit.

## Not claimed

The added Rust tests have **not** been compiled or executed in this executor. Therefore this evidence does not advance `P3-SUP-05`, `P3-SUP-06`, or `P3-SUP-10` from `READY_VERIFY` to `DONE`, and it does not satisfy `P3-V-01..04`, `P3-V-07..10`, `P3-A-01..04`, or `P3-GATE`.

Still required on a Rust-equipped Linux/WSL checkout:

- `cargo fmt --check`
- `cargo check --workspace`
- `cargo clippy --workspace --all-targets -- -D warnings`
- `cargo test --workspace`
- `scripts/v1/check-rust.sh`
- `scripts/v1/check.sh`
- real `session_supervisor_smoke` / `session_inspection_smoke`
- canonical persistent-state, parallel-Session, crash-containment, and hostile-inspection acceptance

Native Windows `scripts/v1/check.ps1` remains additionally required where applicable.

GitHub Actions / GitHub-hosted CI were not used, triggered, inspected, or monitored.
