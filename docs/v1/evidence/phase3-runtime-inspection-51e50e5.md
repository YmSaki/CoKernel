# Phase 3 Runtime inspection hardening evidence

Baseline: `51e50e572d0ac70b78c232c518e9a693627f7aef`

This batch does **not** claim a Rust/WSL Gate PASS. The executor has no `cargo`, `rustc`, `rustfmt`, PowerShell, or Windows/WSL target. GitHub-hosted CI was not used, triggered, or monitored.

## Completed work

1. Closed implementation slice #39 after existing evidence confirmed the worker-side Unix socket protocol, execution path and safe inspection implementation: exact-source worker pytest **137/137 PASS**, source AF_UNIX subprocess smoke PASS, protocol fixtures **5/5 PASS**.
2. Aligned `scripts/v1/check.ps1` with the local verification discipline: explicit python/cargo/uv prerequisites and versions; WBS consistency; local script contract tests; protocol fixtures; source-worker diagnostics; native cargo/uv checks; and mandatory WSL `bash scripts/v1/check.sh` before PASS.
3. Added five static contract tests for the PowerShell gate: ordering, required tools, mandatory WSL production gate, optional distro selection, and absence of GitHub Actions invocations. Local reconstructed unittest result: **5/5 PASS**.
4. Hardened Runtime `list_variables` results: max result bytes/items, bounded non-empty metadata, duplicate variable-name rejection before exposure to callers.
5. Hardened Runtime `get_variable` results: required field presence, response-size and metadata bounds, request/result name correlation, `supported`/`value`/`reason` consistency, including explicit distinction between present JSON `null` (valid supported Python `None`) and an omitted `value` field.
6. Added eight focused Rust contract tests for the new Runtime inspection validators.
7. Extended `session_inspection_smoke` so the future real WSL run proves supported Python `None` semantics in addition to primitive/custom-object inspection and worker survival.

## Review correction before Gate

Post-commit diff review caught a Rust borrow-checker defect in the first validator implementation: the code borrowed `result.as_object()`, moved `result` into `serde_json::from_value`, then reused the object borrow. Follow-up commit `616ccaad` captures the raw-null invariant before moving the JSON value.

## Verification actually performed

- PowerShell gate contract tests: **5/5 PASS** using the exact reconstructed script/test content.
- Rust source/test delimiter sanity: braces, parentheses and brackets balanced for modified `session.rs` and `inspection_contract_tests.rs`.
- GitHub compare from baseline shows only the intended five paths plus this evidence file.
- #39 was closed as completed; #40 and #29 remain open.

## Still required

On a Rust-equipped Linux/WSL checkout:

- `cargo fmt --all --check`
- `cargo check --workspace --all-targets`
- `cargo clippy --workspace --all-targets -- -D warnings`
- `cargo test --workspace`
- `scripts/v1/check.sh`
- real `session_supervisor_smoke` and `session_inspection_smoke`
- Phase 3 canonical persistent-state/concurrency/crash/hostile-inspection acceptance.

On Windows, run `scripts/v1/check.ps1`; it must also complete the WSL production-path gate before reporting PASS.
