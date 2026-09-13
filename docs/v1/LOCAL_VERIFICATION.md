# CoKernel v1 local verification

Status: implementation support policy

GitHub Actions is useful evidence when available, but CoKernel v1 development must not depend on hosted CI availability, billing status, or quota.

## Canonical local checks

Linux/WSL:

```bash
./scripts/v1/check.sh
```

Windows PowerShell:

```powershell
./scripts/v1/check.ps1
```

These entrypoints run the current baseline checks:

- `cargo fmt --all --check`;
- `cargo check --workspace --all-targets`;
- `cargo clippy --workspace --all-targets -- -D warnings`;
- `cargo test --workspace`;
- protocol fixture validation;
- worker uv environment sync;
- worker executable smoke test;
- worker pytest suite;
- uv Project + CoKernel worker overlay spike;
- Linux/WSL worker SIGINT survival/continuation spike.

The SIGINT spike intentionally skips on native Windows because the production Python worker runs inside WSL/Linux and is supervised there.

## Evidence policy

- A hosted-CI failure caused by code/test failure is a blocker and must be investigated.
- A hosted-CI run that does not start, is cancelled for quota/billing, or is unavailable is **not** by itself a code failure.
- When hosted CI is unavailable, the implementation log/issue comment should record which local checks were run and their result.
- Real-machine Windows/WSL/NVIDIA acceptance remains mandatory for the release gates in `TEST_ACCEPTANCE.md`; hosted CI cannot replace it.

## Adding checks

Every new phase should add its deterministic developer checks to these local entrypoints first, then mirror them into GitHub Actions when practical. This keeps the verification path usable even when hosted CI is unavailable.
