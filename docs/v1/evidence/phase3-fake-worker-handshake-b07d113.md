# Phase 3 fake-worker / handshake hardening evidence

Base branch state: `b07d113813d3647893b0877fc3b5829d019c42c5`

Commits in this batch:

- `0fe99c1` / `63fea8d` / `131a40d` / `82ab01d` — add and harden a Unix-only fake-worker Session Supervisor component test harness. It now covers heartbeat timeout, survival of an unrelated healthy Session, unexpected idle transaction fail-closed behavior, missing required handshake capability, and ready-PID mismatch against Unix peer credentials.
- `5ae70ca` — harden Runtime worker-handshake validation: bound worker version/capability metadata, reject duplicate/malformed capability lists, require coherent advertised output limits, bound failed-handshake diagnostic fields, and stop including arbitrary worker payload/result content in unexpected-handshake diagnostics.
- `5813843` / `8b0c202` — extract the fake uv/worker into `tests/fixtures/fake_uv.py` and reuse it from the Rust component tests so its Python syntax can be checked independently of the unavailable Rust toolchain.

New component coverage maps directly to `TEST_ACCEPTANCE.md` L2 Session Supervisor requirements:

- start / ready / explicit handshake trust boundary;
- heartbeat timeout;
- FailureRecord trigger evidence;
- one Session failure does not affect an unrelated Session;
- stale/unexpected transaction frames fail closed;
- Unix peer PID and worker-reported PID must agree.

Local non-hosted verification available in this executor:

- fake worker Python fixture: `python -m py_compile` **PASS**;
- commit/diff review confirmed the branch changes are limited to the fake-worker component test/fixture and handshake hardening plus this evidence record;
- GitHub Actions / GitHub-hosted CI were not used, started, or monitored.

Not verified here:

- `cargo fmt --all --check`;
- `cargo check --workspace --all-targets`;
- `cargo clippy --workspace --all-targets -- -D warnings`;
- `cargo test --workspace` (including `session_fake_worker`);
- `scripts/v1/check.sh` / `scripts/v1/check.ps1`;
- real WSL Session Supervisor smoke.

Reason: this executor still has no `cargo`, `rustc`, `rustfmt`, PowerShell, Windows, or WSL target. These additions therefore remain verification support for the existing Phase-3 `READY_VERIFY` rows; they do not close #29/#40 or permit Phase 4 to start.
