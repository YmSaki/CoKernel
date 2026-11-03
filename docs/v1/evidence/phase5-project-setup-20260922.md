# Phase 5 asynchronous Project setup evidence — 2026-09-22

Base: `f35ca99711a8d6c102ef9e47b857e0e377f87cbb` plus the working tree.
This records progress within **P5-07**, which remains IN_PROGRESS for long
Session startup/inspection dispatch. It does not close P5-GATE, Desktop/MCP/JAX
GPU acceptance or the delivery goal. No commit, PR or issue closure was performed.

## Implementation

- Project create/register now return PROJECT operation records before waiting
  for uv. They share package operations' per-Project exclusion, global capacity,
  cancellation, retained history, completion events and bridge-EOF cleanup.
- New creation reserves directory/identity and persists incomplete state before
  running init/sync. Failed/cancelled setup preserves files and identity.
- `initialization_required` survives Runtime restart. `packages.sync` resumes
  incomplete initialization, running init only when pyproject is absent, then
  sync. Existing declarations and user files are retained. New Session startup
  remains unavailable while the environment is uncertain.
- Registration validates the managed existing path. Re-registration synchronizes
  the same Project ID and cannot overlap another mutation for that Project.
- Capacity rejection occurs before directory reservation. Failed registry
  admission launches no uv and leaves a visible uncertain identity for repair.
  If storage never accepted the registry write, the ID is not guaranteed after
  restart; the API reports failure and does not claim successful creation.
- The Runtime advertises `async-projects`; actual Runtime smoke clients and both
  Host examples now wait for setup completion before starting Sessions.

The wire and recovery contract is documented in INTERFACE_CONTRACTS section 21.
The standalone Project CLI retains synchronous command semantics.

## Actual Runtime / uv / worker proof

The canonical Linux gate includes `scripts/v1/smoke_async_projects.py`. A private
test wrapper delays actual uv init/sync at a barrier; it delegates to the real uv
binary when released. The proof verifies:

1. Project creation returns an operation ID within two seconds while init is held;
   another Project's running Session remains available through status/control.
2. Duplicate creation fails. Cancellation reaps uv without deleting an externally
   placed user file. The cancelled Project retains initialization/uncertainty flags.
3. Sync resumes the same Project, creates its Python environment and preserves
   the user file.
4. Registering an existing pyproject likewise returns immediately while uv sync
   is held. Re-registration while busy is refused. An unrelated Session interrupt
   responds within two seconds, and its persistent marker remains 123456789.
5. Registration completion is observed from an event without polling; subsequent
   registration reuses the same Project identity.
6. EOF during another creation cancels/reaps uv. After Runtime restart, the same
   incomplete Project ID and flags are present; sync completes initialization.

Rust tests additionally cover shared capacity with package work, no duplicate
registry entries, failed registry admission without uv launch and successful repair
after restoring registry storage. The earlier package cancellation, failed-runner,
path/argument and registry-commit-failure tests remain in the same suite.

## Actual Windows Host proof

The updated `wsl_pipe_smoke` observes Project creation completion through a real
Windows Named Pipe, Host, WSL Runtime and uv. It does not poll for that completion.
The existing shared primary Session, accepted execution after client disconnect,
chunked Windows import, package completion, document output, dirty shutdown flush
and reopen checks also pass.

Final isolated Linux root:
`/home/ymsaki/.cache/cokernel-v1-dev/host-proof-6410b26d2657`.
An independent WSL verifier validates saved nbformat, output 124, attachment and
shutdown metadata; it confirms failed import left no file and the worker PID is gone.

These are Rust clients, not actual Desktop, Codex or web ChatGPT clients. The
development Ubuntu distro does not prove managed-distro security or JAX GPU use.

## Final verification

| Check | Result |
|---|---|
| Windows check.ps1 including actual WSL check.sh | PASS, exit 0 |
| Native Rust workspace | 76 unique tests PASS; fmt/check/clippy PASS |
| WSL Rust workspace | 170 unique tests PASS; fmt/check/clippy PASS |
| Native worker | 163 PASS, eight Unix-only skips |
| WSL worker | 171 PASS |
| Real Project/worker/Supervisor/inspection/notebook/package/setup smokes | PASS |
| Windows/WSL source identity before/after gate | 115 files match |
| Actual Windows pipe/Host/WSL proof | PASS, exit 0 |
| Independent notebook/worker-exit verification | PASS, exit 0 |

The credential-isolation test's nested subprocess is not counted twice. Existing
platform-specific skips remain. No acceptance timeout or lint rule was weakened.
Documentation-only evidence/ledger updates follow compiled-source verification.

Artifacts: [combined gate](phase5-project-setup-20260922.gate.txt),
[actual Host and independent postconditions](phase5-project-setup-20260922.real.txt),
[source hashes](phase5-project-setup-20260922.sources.json).
Reproduction uses the established source-identity Windows dispatcher and actual
Linux gate. For the Windows proof, use the existing isolated Host preparation
helper and a fresh Host state directory with the current Runtime build.

## Next control-dispatch work

Session startup and inspection still wait inside serial Runtime RPC admission.
Their response waits must be detached while keeping bounded requests, domain
ownership, single-primary identity, execution ordering and Project mutation/startup
exclusion. Merely spawning the whole mutable Runtime service would not establish
those invariants. P5-07 remains open until that path has actual control evidence.
