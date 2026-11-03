# Phase 3 local Windows / WSL verification, 2026-09-21

Base: `f35ca99711a8d6c102ef9e47b857e0e377f87cbb` plus the working-tree repairs
listed below. No GitHub-hosted CI was used.

## Source identity and execution locations

Windows source: `C:\programing\CoKernel`.
WSL Ubuntu test snapshot: `/home/ymsaki/.cache/cokernel-v1-dev/repo`.
All 168 files in the snapshot inventory matched the Windows working tree by
SHA-256 after the complete Linux gate passed. See
[source hashes](phase3-local-20260921.sources.json). Subsequent ledger, planning
and evidence-only edits are not part of that snapshot; executable code is bound
by the recorded hashes. This is a complete source snapshot, not a Git checkout.

The subsequent combined Windows gate refreshed the snapshot and checked all
170 source files both before and after the Linux invocation. Both comparisons
passed. The executable sources and dependency locks are unchanged from the
168-file hash record; the additional files were evidence documents. Final
ledger/evidence-only updates follow the run and are separately count-validated.

Tool versions:

- Windows: Rust/Cargo 1.91.1, Python 3.11.9, uv 0.12.11.
- Windows worker environment: managed Python 3.12.14, 163 tests PASS and four
  Unix-only tests skipped; the Linux gate passes all 167 without skips.
- WSL: Rust/Cargo 1.88.0, rustfmt 1.8.0, managed Python 3.12.14, uv 0.12.11,
  IPython 9.17.1, pytest 9.1.1.
- Cargo.lock and worker/uv.lock now record the resolved dependencies.

The Linux gate used PATH entries for the snapshot worker venv, the private
development uv binary and the existing Rust toolchain. UV_CACHE_DIR and
UV_PYTHON_INSTALL_DIR pointed to the private Linux development cache. System
Python was not replaced. This developer Ubuntu environment is not an installed,
hardened CoKernel product distro.

## Repairs verified

1. Windows check.ps1 checks every native command's exit status immediately.
   Behavioral regression injects exit 7 at each of 19 stages and verifies there
   is no subsequent stage or PASS. A success case reaches every stage including
   the mandatory WSL invocation. The test explicitly disables PowerShell native
   error promotion to exercise the previously failing configuration.
2. Worker pytest explicitly targets worker/tests. `uv --project` selects the
   environment but does not change pytest's working-directory discovery.
3. Fixed a Unix-only MutexGuard dereference compilation error, applied formatting,
   reduced the large event variant and grouped transaction borrows for clippy.
4. Shortened temporary socket paths. The previous fake-worker suite failed all
   16 tests before startup because paths exceeded the Unix socket name limit.
5. FailureStore persists immutable bounded records independently of subscribers.
   Tests cover reopening, retention, session filtering, write failure, malformed/
   oversized/schema-mismatched/identity-mismatched records, Unix permissions and
   symlinks. Live failure status exposes persistence errors.
6. A private owned process group terminates ordinary descendants on stop,
   restart, crash after leader exit and rejected startup. Tests create actual
   child Python processes, including a child ignoring SIGTERM, and prove another
   Session remains usable. Zombies awaiting OS reaping are treated as exited;
   this does not claim containment of deliberately detached processes.

## Successful verification

| Platform / command | Result |
|---|---|
| Windows script unittest discovery | 21 tests PASS, including 19 injected failure stages plus success path |
| Windows cargo fmt --all --check | PASS |
| Windows cargo check --workspace --all-targets | PASS |
| Windows cargo clippy --workspace --all-targets -- -D warnings | PASS |
| Windows cargo test --workspace | 29 tests PASS; Unix Session tests correctly excluded |
| WSL scripts/v1/check.sh | PASS through final `[v1-check] PASS` |
| WSL worker pytest | 167 PASS in 3.29 seconds, no skips |
| WSL shared script tests | 19 PASS, 2 Windows PowerShell execution tests skipped and separately passed on Windows |
| WSL protocol fixtures | 5 PASS |
| WSL source worker AF_UNIX smoke | PASS |
| WSL uv worker subprocess smoke | PASS |
| WSL Project + product worker overlay spike | PASS |
| WSL real-worker SIGINT survival spike | PASS |
| WSL cargo fmt/check/clippy | PASS |
| WSL cargo test --workspace | 97 PASS: domain 2, MCP scaffolding 3, protocol 4, Runtime unit 67, Session process 21 |
| WSL Runtime Project + uv smoke | PASS |
| WSL Session Supervisor canonical smoke | PASS |
| WSL Session safe-inspection smoke | PASS |
| Combined Windows check.ps1 + mandatory WSL check.sh | PASS, exit 0; development dispatch arrangement below |

The Supervisor smoke proves the real Runtime -> uv -> IPython path: persistent
state, shared Human/MCP-origin FIFO, distinct concurrent Sessions, interruption
with namespace survival, explicit restart/stop, environment-generation warning,
forced worker exit with correlated evidence, and unrelated-Session survival.
The origins in this test are simulated callers, not a real MCP server or Desktop.
Actual uv mutation -> public RuntimeService -> stale notification is a later
service-integration task in DELIVERY_PLAN D06; the Supervisor warning behavior
alone does not close that full product path.

## Failed attempts and limits

- The initial offline Windows cargo check lacked the required rmcp cache; the
  networked dependency resolution and later builds succeeded.
- The original Unix code failed compilation and its component tests had invalid
  socket paths; these failures were repaired rather than waived.
- The first pytest gate incorrectly collected legacy mcp-extension tests and
  developer files: 242 collected / 1 import error after 502.93 seconds. Explicit
  worker/tests selection fixes the discovery error.
- Running managed Python/uv caches from `/mnt/c` caused substantial I/O overhead
  and a real-worker startup timeout. Native Linux placement passed without
  increasing the production 15-second startup timeout.
- `/tmp` developer assets disappeared between WSL invocations after long-running
  test processes ended. Persistent developer assets were moved under the Linux
  home cache. Product Host/WSL lifetime management is still Phase 5 work.
- A final benign `/bin/kill` "No such process" diagnostic appeared during the
  crash smoke; the complete correlated crash and survivor assertions passed.
- The first combined Windows attempt could not replace a Linux-generated
  worker/.venv symlink using native Windows uv. That disposable development
  environment was moved under target/dev-tools; Windows created its own venv.
  The Linux snapshot retains an independent native Linux venv.
- Development dispatcher setup also exposed a PowerShell variable-scope error,
  multiple wsl.exe command resolutions and lost shell executable bits after
  extraction of the Windows archive. These setup errors were corrected before
  the successful complete run; none was waived or converted into a skip.
- Notebook persistence, real Host/MCP/tunnel/Desktop, installer/update/repair,
  actual Codex/web ChatGPT participation and JAX GPU acceptance remain unproven.

## Combined Windows dispatch arrangement

The normal check.ps1 body ran unchanged, including every native Windows check
and its final mandatory WSL gate. A development-only PowerShell wsl.exe function
validated the exact requested arguments (`bash scripts/v1/check.sh`), then
invoked the actual first resolved Windows wsl.exe application in Ubuntu. It
selected the byte-identical native Linux snapshot and the development tool PATH
and caches described above. A Python SHA-256 comparison of the snapshot against
the Windows inventory ran before and after the actual Linux check.sh. Any
comparison, Linux gate or native Windows failure propagated as a nonzero exit.
This was real WSL execution, not a stand-in returning success.

The dispatcher is local development setup, not production Host/WSL behavior.
An unconfigured bare check.ps1 invocation is not claimed to work on this machine:
the default WSL PATH lacks the private uv install and /mnt/c venv/cache placement
had already failed the production startup-time budget. No timeouts were relaxed.

The complete successful output is retained in
[Windows and WSL gate output](phase3-local-20260921.windows-output.txt).
The exact development dispatcher and identity comparison used are retained in
[dispatcher](phase3-local-20260921.runner.ps1) and
[comparison](phase3-local-20260921.compare.py). Their machine-local paths document
this run; they are not installed product components.

## Next work

The local Phase 3 gate is now complete. GitHub issue state has not been changed.
Begin Phase 4 notebook model/document work. Preserve existing tests and limits;
simulated AI callers cannot replace the agreed final three-client acceptance.
