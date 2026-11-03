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
- Linux/WSL worker SIGINT survival/continuation spike;
- Linux Session Supervisor and hostile safe-inspection smoke tests;
- Linux framed Runtime notebook import/execute/persist/close/stale-environment
  smoke, including independent nbformat validation and oversized response recovery.

The notebook smoke uses `uv run --no-project --with nbformat==5.11.1`; nbformat is
a test-only validator and is not a product Runtime or Project dependency. It runs
an isolated temporary Project and cleans up its own Runtime/worker processes.

The Linux gate also runs `python3 scripts/v1/smoke_async_packages.py`. It holds a
real uv add behind a test barrier while checking Session interrupt/status and
namespace survival, then verifies event-driven completion, partial-frame input,
explicit package cancellation and uv cleanup/registry reconciliation on EOF.
`python3 scripts/v1/smoke_async_projects.py` covers asynchronous Project creation
and registration with real uv, unrelated Session interrupt, retained user files,
sync-based initialization recovery, and EOF/restart recovery of incomplete setup.
`python3 scripts/v1/smoke_session_rpc.py` checks concurrent notebook startup,
deduplicated primary creation, bounded pending inspection, responsive interrupt,
out-of-order replies, restart/stop cancellation and EOF worker cleanup using
actual uv/IPython processes.
The same smoke now checks two notebooks in one Project: matching Project package
origin, separate worker PIDs/namespaces, overlapping execution and unchanged
dependency truth. Runtime endpoint component tests exercise caller correlation,
bounded unread replies, disconnection, revision conflicts and owner shutdown.
Linux bridge-input tests also require executor exit with a peer writer still open;
the Project recovery smoke exercises explicit shutdown without closing stdin.
Document-owner component tests hold filesystem access behind a barrier while
another Session answers status and interrupt. They also prove preparation FIFO
with a cancelled middle request, lifecycle/package exclusion, stop cancellation,
bounded queue admission, accepted writes after caller disconnect, recovery after
a failed job, and shutdown refusal followed by successful storage recovery.
Session control tests keep the actor unpolled while submitting an interrupt burst,
Project stale notification and stop; all must finish on their first poll. Another
Notebook in that Project must still answer through RuntimeHandle and execute
successfully. Control-unit tests verify bounded coalescing and stop priority.
Project-owner tests hold storage before uv launch and during post-uv reconciliation,
then verify unrelated Session control, durable uncertainty, retained reservations,
registration alias exclusion, failed registry-removal retry, capacity rejection,
owner failure and EOF without a late uv launch.

The SIGINT spike intentionally skips on native Windows because the production Python worker runs inside WSL/Linux and is supervised there.

Phase 5 Windows integration additionally uses
`cargo run -p cokernel-host --example wsl_pipe_smoke -- <isolated-wsl-config.json> <absolute-isolated-state-dir>`.
This starts a real current-user Named Pipe and Host, creates a Windows notebook
larger than one import chunk, transfers it to a disposable WSL Project, executes
it, reconnects independent clients and checks shutdown/reopen behavior. It must
observe live execution/output/document-completion events without polling and
verify that a reconnect snapshot contains the same primary Session. It must
also observe a pending worker inspection during execution and serve another
client's snapshot/status request before that inspection finishes. It must
use a fresh Project root supplied by the test Runtime configuration. Follow with
`uv run --no-project --with nbformat==5.11.1 python scripts/v1/verify_host_pipe_smoke.py <native-output-log> <linux-project-root-file>`
inside WSL to independently validate saved nbformat/output/attachment and worker
exit. This is explicit machine acceptance in addition to check.ps1, not an actual
Desktop or MCP client proof. Concrete development paths are recorded in evidence.

Phase 6 starts with `cargo test -p cokernel-mcp --all-targets` inside Linux,
including official SDK HTTP interoperation and private Unix/authentication
regressions. `scripts/v1/check-rust.sh` also runs
`python3 scripts/v1/smoke_mcp_lifecycle.py`: a disposable uv Project and actual
Runtime/MCP/IPython processes prove MCP SIGKILL/restart leaves the existing
primary Session, worker PID and variable state intact. The script owns and cleans
its child processes and temporary credentials, and never prints the bearer.
The current smoke covers all fifteen tools, the canonical shared value, shared
FIFO, parallel isolated Notebooks, ownership/revision rejection, hostile values,
large-output bounds and Session controls. The transport snapshot is generated
explicitly with `cargo run -q -p cokernel-mcp --example tool_contract` and reviewed
against `protocol/mcp/tools.json`; tests never rewrite the snapshot.
`python3 scripts/v1/smoke_mcp_supervision.py` additionally proves Runtime-owned
recovery, retry exhaustion, manual MCP-only recovery and SIGSTOP health recovery
with stable endpoint/bearer and surviving live worker state. It is part of the
canonical Linux gate. Actual Codex/Web ChatGPT access and managed control/workload
identity separation remain separate acceptance requirements.

## Evidence policy

- A hosted-CI failure caused by code/test failure is a blocker and must be investigated.
- A hosted-CI run that does not start, is cancelled for quota/billing, or is unavailable is **not** by itself a code failure.
- When hosted CI is unavailable, the implementation log/issue comment should record which local checks were run and their result.
- Real-machine Windows/WSL/NVIDIA acceptance remains mandatory for the release gates in `TEST_ACCEPTANCE.md`; hosted CI cannot replace it.

## Adding checks

Every new phase should add its deterministic developer checks to these local entrypoints first, then mirror them into GitHub Actions when practical. This keeps the verification path usable even when hosted CI is unavailable.
