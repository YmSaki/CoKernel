# Phase 3 Runtime boundary hardening evidence

Base branch state: `c5ef59c4796d370a7b32a306d3f6881da52e3ece`

Commits in this batch:

- `d6a160b811c4799b5919d001375b23e47ae58a82` — preflight Session inspection request frames before they enter the actor/worker transport path. Oversized caller-controlled inspection payloads now return `SessionInspectionError::InvalidRequest` instead of turning a local encode failure into a Session crash.
- `bd33275a433913f670692b4c319d93626677bd8d` — extend the real Runtime inspection smoke so an oversized `get_variable` request must be rejected locally and the same persistent Session must remain executable afterward.
- `e9b6b02fc15dfda3fe02643afec1e645b8966f8d` — bound crash diagnostic detail before storing it in `RuntimeFailureContext` and append it through the already-bounded diagnostic `ByteTail`, preventing a protocol/runtime error string from bypassing the configured FailureRecord tail bound. Added Unicode-safe detail truncation and diagnostic-tail regression tests.

Local non-hosted verification available in this executor:

- `scripts/v1/test_check_ps1_contract.py`: **5/5 PASS** against the committed `check.ps1` contract. This proves the native Windows slice requires python/cargo/uv, keeps the WSL `check.sh` production gate mandatory, preserves optional distro selection, orders local diagnostics before toolchain/uv gates, and does not invoke GitHub Actions.
- `scripts/v1/validate_protocol_fixtures.py`: **5/5 fixtures PASS** using the exact committed fixture contents.
- Commit diffs were re-read after each write for structural/regression review.

Not verified here:

- `cargo fmt --all --check`
- `cargo check --workspace --all-targets`
- `cargo clippy --workspace --all-targets -- -D warnings`
- `cargo test --workspace`
- `cargo run -q -p cokernel-runtime --example session_supervisor_smoke`
- `cargo run -q -p cokernel-runtime --example session_inspection_smoke`
- native Windows + WSL `scripts/v1/check.ps1`

Reason: this executor has no `cargo`, `rustc`, `rustfmt`, PowerShell, Windows, or WSL target. The new Rust work therefore remains `READY_VERIFY`; it is not acceptance evidence for Phase 3 closure.

GitHub Actions / GitHub-hosted CI were not used, started, or monitored.
