# CoKernel v1 Work Status

Purpose: execution ledger for `cokernel-v1`. Authoritative requirements remain `IMPLEMENTATION_ORDER.md`, `docs/v1/*`, and Issues #26-#35. Update this file when a phase gate changes, not for every small commit.

## Phase tracker

| Phase | Issue | State | Gate / dependency | Current action |
|---|---:|---|---|---|
| 0 | #27 | DONE | feasibility spikes complete | none |
| 1-2 | #28 | DONE | domain/protocol + uv Project manager complete | none |
| 3 | #29 | ACTIVE | must pass Session execution/concurrency/crash-containment gate before Phase 4 | finish protocol/session invariants, then run canonical local/WSL gate |
| 4 | #30 | BLOCKED | depends on Phase 3 gate | notebook document service |
| 5 | #31 | BLOCKED | depends on lower Runtime contracts | Windows Host / WSL bridge |
| 6-7 | #32 | BLOCKED | depends on Runtime + Host contracts | native MCP + tunnel |
| 8 | #33 | BLOCKED | depends on Runtime/Host APIs | Desktop |
| 9-10 | #34 | BLOCKED | depends on product runtime shape | installer/update/repair |
| 11 | #35 | BLOCKED | depends on all prior gates | full acceptance |

Umbrella: #26 remains open until v1 release acceptance is complete.

## Active phase: Phase 3 / #29

### Implemented baseline

- product-controlled Python/IPython worker; no Jupyter/ipykernel/Jupyter Server in the v1 execution path;
- private framed worker protocol;
- persistent IPython namespace and bounded output capture;
- safe variable inspection primitives;
- Session Supervisor with per-Session FIFO execution;
- interrupt/restart/stop and crash/failure evidence paths;
- independent concurrent Sessions;
- worker-frame shape validation and active-transaction correlation;
- execute lifecycle validation (`execution_started` -> contiguous outputs -> `execution_finished` -> response);
- oversized request preflight so a caller error does not crash a healthy Session;
- output normalization/transport failures converted into bounded FAILED outcomes while preserving worker/session liveness;
- worker JSON transport rejects lossy non-string object-key coercion;
- user exception names are bounded/non-empty before crossing the protocol boundary.

### Remaining implementation audit

1. Final execute response must bind its result `operation_id` exactly to the active Runtime operation, not merely validate it as non-empty.
2. `execution_finished` truncation accounting must be self-consistent (`output_truncated`, `output_omitted_bytes`, `output_truncation_reasons`) and agree with the final response.
3. Any newly found Phase-3 contract hole is fixed fail-closed with a focused regression test before moving to Phase 4.

### Verification debt before closing #29

Run on a Rust-equipped Linux/WSL checkout, without GitHub-hosted CI:

- `cargo fmt --check`;
- `cargo check --workspace`;
- `cargo clippy --workspace --all-targets -- -D warnings`;
- `cargo test --workspace`;
- worker pytest;
- protocol fixture validation;
- `scripts/v1/check.sh` (and Windows-side `scripts/v1/check.ps1` where applicable);
- canonical Session proofs: persistent `x=123` -> `x+1 == 124`, parallel isolated A/B Sessions, force-kill A while B survives and A records FailureRecord;
- supervisor/inspection smoke tests on real Linux/WSL process/socket behavior.

## Working rule

Always resume from the first unfinished row above. Within the active phase, consume `Remaining implementation audit` top-to-bottom. Do not re-audit completed phases unless a regression or specification change points back to them. GitHub Actions / GitHub-hosted CI are not part of the v1 validation path.
