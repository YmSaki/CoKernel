# Phase 3 fake-worker execution component evidence

Date: 2026-09-15

Base revision: `8625e0e744f1086972e0a7981578de614d4f9818`

Implementation revisions:

- `c41e80e77df61c45360f1056fe475d32d827becf` — extend the fake worker with bounded execute lifecycle, heartbeat during execution, SIGINT handling, cooperative shutdown, and deterministic exit-23 crash mode.
- `a111aa9035836133eed32085cc6565d080ecf36c` — add Session Supervisor component tests for shared FIFO ordering, interrupt survival, forced worker crash correlation/isolation, and concurrent Sessions.

## Scope

This slice strengthens Phase 3 component coverage without crossing the Phase 3 gate and without adding Desktop/MCP/Jupyter Server execution paths.

The new Rust integration tests exercise the implementation behind:

- `P3-SUP-03`: Human and MCP execution requests share one per-Session FIFO and finish in accepted order; queue depth reflects the queued second operation while the first is executing.
- `P3-SUP-04`: SIGINT marks the active Runtime operation `INTERRUPTED`, returns the Session to `IDLE`, and a subsequent execution succeeds in the same worker.
- `P3-SUP-08`: a worker that exits during an active operation crashes only its own Session; another Session remains usable.
- `P3-SUP-09`: the crash `FailureRecord` is correlated to the active `operation_id`, carries the Session ID, and records exit code 23.
- `P3-SUP-11`: two independent Session actors execute concurrently; a fast operation in Session B completes while Session A remains `EXECUTING`.

The fake worker emits the same minimum lifecycle required by the Runtime transaction validator: `execution_started`, optional heartbeats, `execution_finished`, and the final execute response with matching operation/status/execution/truncation accounting.

## Source identity

Committed Git blob IDs after the implementation commits:

- `crates/cokernel-runtime/tests/fixtures/fake_uv.py`: `20f35c12e7410f9326b30b6950dcee1eb6db7785`
- `crates/cokernel-runtime/tests/session_fake_worker.rs`: `1f970e666b7b2b0c043a9a76a66d1c75825cd25e`

The branch comparison from `8625e0e` to `a111aa9` contains exactly two modified files: the fake worker fixture and the Session Supervisor integration test.

## Non-hosted verification performed

Available executor environment:

- Python `3.13.5`
- `uv` available
- no `cargo`
- no `rustc`
- no `rustfmt`
- no connected Windows/WSL CoKernel execution target

Checks performed against the exact fake-worker source committed as blob `20f35c12...`:

1. `python3 -m py_compile` — **PASS**.
2. `git hash-object` — **PASS**, exactly `20f35c12e7410f9326b30b6950dcee1eb6db7785`.
3. Real AF_UNIX subprocess interrupt/survival smoke — **PASS**:
   - ready PID matched the spawned process;
   - handshake advertised `execute`;
   - a long execute emitted `execution_started`;
   - real `SIGINT` caused a terminal `FAILED` worker lifecycle;
   - a second execute in the same worker completed `SUCCEEDED`;
   - cooperative shutdown exited 0.
4. Real AF_UNIX subprocess crash smoke — **PASS**:
   - execute emitted `execution_started`;
   - fake worker exited with code 23;
   - socket closed as expected.
5. Two-process parallel smoke — **PASS**:
   - Session-B-style fast execute completed while the Session-A-style slow worker was still running;
   - both workers subsequently completed successfully and shut down cleanly.
6. Rust integration-test source was reviewed against the current `SessionHandle` / `SessionEvent` / domain enum APIs and the Runtime lifecycle contract. This is a static review only, not a compile/test result.

## Not claimed

The Rust integration tests added in `a111aa9` have **not** been compiled or executed in this executor. Therefore this evidence does not advance `P3-SUP-03`, `P3-SUP-04`, `P3-SUP-08`, `P3-SUP-09`, or `P3-SUP-11` from `READY_VERIFY` to `DONE`.

It also does not satisfy:

- `P3-V-01` `cargo fmt --check`
- `P3-V-02` `cargo check --workspace`
- `P3-V-03` `cargo clippy --workspace --all-targets -- -D warnings`
- `P3-V-04` `cargo test --workspace`
- `P3-V-07..10` canonical Linux/WSL/Windows/runtime smokes
- `P3-A-01..04` acceptance
- `P3-GATE`

Still required on a Rust-equipped Linux/WSL checkout: `scripts/v1/check-rust.sh`, `scripts/v1/check.sh`, real `session_supervisor_smoke` / `session_inspection_smoke`, and canonical persistent-state / parallel-Session / crash-containment / hostile-inspection acceptance. Native Windows `scripts/v1/check.ps1` remains additionally required where applicable.

GitHub Actions / GitHub-hosted CI were not used, triggered, inspected, or monitored.
