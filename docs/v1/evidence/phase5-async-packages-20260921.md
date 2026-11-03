# Phase 5 asynchronous package operations evidence — 2026-09-21–22

The artifact identifier retains the batch start date; evidence recording completed
on 2026-09-22 (Asia/Tokyo).

Base: `f35ca99711a8d6c102ef9e47b857e0e377f87cbb` plus the working tree.
This records progress within **P5-07**, which remains IN_PROGRESS. It does not
close P5-GATE, Desktop/MCP/JAX GPU acceptance or the overall delivery goal.
No commit, PR or issue closure was performed.

## Implementation boundary

1. Project Manager separates package validation/admission, uv execution and
   registry reconciliation. The authoritative registry remains owned by Runtime;
   only the validated uv invocation and shared runner enter a blocking task.
2. Add/remove/sync return a PACKAGE operation ID immediately. Runtime allows one
   mutation per Project, four globally and 256 retained operation records. It
   preserves Project ownership on get/cancel and never evicts running records.
3. The bridge handles package completion alongside input reads. It preserves a
   partially read frame and commits a taken completion without cancellation.
   Terminal progress is published without requiring another RPC request.
4. Production uv has bounded time/output, null stdin and a private Linux process
   group. Cancellation and EOF terminate/reap uv and ordinary descendants before
   reconciliation. Cancellation is separate from Session interruption.
5. An atomic write-ahead uncertainty marker prevents new Session startup against
   an unconfirmed environment after Runtime restart. Failed attempts conservatively
   advance generation; live Session memory survives. Existing primary Session
   lookup remains available while package mutation runs. A successful sync/change
   clears uncertainty; explicit Session restart adopts the new environment.
6. Host forwards operations.cancel and exposes the connected Runtime's capability
   list/generation. Its real pipe example now verifies an asynchronous package
   completion event, operation lookup and terminal cancellation response.

See INTERFACE_CONTRACTS section 20 for bounds, result states, cancellation races,
generation semantics and remaining synchronous paths. No second dependency
resolver/store was introduced.

## Evidence that changed the implementation

Review identified that a registry commit error could leave the in-memory Project
marked healthy even though its operation failed. A new test replaced the isolated
registry destination with a directory after admission. It failed with
`failed registry commit must not make the in-memory environment ready`.
The repair retains uncertainty in memory and attempts to restore the persistent
marker. The same test passes in the final full gate.

Other development checks caught a missing timeout import in the Windows example
and a test-only mutex held across an injected runner panic; both were corrected.
No acceptance condition, timeout or lint rule was relaxed to obtain PASS.

## Real Runtime / uv / IPython proof

The canonical Linux gate runs `scripts/v1/smoke_async_packages.py`. A private test
wrapper delays uv add but eventually delegates to the actual uv binary. While
the package operation is held, the proof verifies:

- admission returns an operation ID within two seconds;
- ping, operation status, duplicate rejection and Session interrupt respond within
  two seconds, before uv is released;
- existing primary Session lookup returns the same ID; restart is refused while
  its Project is mutating;
- Python interruption persists its terminal output and the same Session retains
  `secret_from_master=123456789`;
- releasing uv publishes terminal progress with no complete new request or poll,
  even while only two bytes of the next bridge header have arrived;
- the remaining frame decodes correctly, the Session is stale and memory survives;
- explicit cancellation reaps uv, produces CK-UV-CANCELLED, retains memory and
  requires synchronization before restart;
- EOF during another package operation cancels/reaps uv, flushes/stops Runtime
  and leaves the registry at the expected conservative generation.

Rust tests cover parallel admission/capacity, ownership, history eviction,
failed/panicked uv, write-ahead marker on registry reopen, registry commit failure,
option argument rejection and symlink escape. Process tests cover time/output
bounds and a descendant holding an output pipe after its parent exits.

## Actual Windows Named Pipe / Host / WSL proof

`wsl_pipe_smoke` passes with a fresh isolated Project root:
`/home/ymsaki/.cache/cokernel-v1-dev/host-proof-f46dab82b543`.

The observer verifies `async-packages` in Runtime capabilities, observes a package
sync's terminal event without polling, reads the operation and obtains
`accepted:false` when cancelling that completed operation. Stopping Runtime clears
the capabilities. The earlier multi-client shared Session, chunked Windows import,
execution event, output persistence, dirty shutdown flush and reopen proofs also
pass. An independent WSL verifier validates saved nbformat/output/attachment and
confirms the worker PID is gone.

These are Rust clients and a development Ubuntu environment. They do not replace
actual Desktop/Codex/ChatGPT, managed-distro security or JAX GPU acceptance.

## Final verification

| Check | Result |
|---|---|
| Canonical Windows check.ps1 including actual WSL check.sh | PASS, exit 0 |
| Native Rust workspace | 76 unique tests PASS; fmt/check/clippy PASS |
| WSL Rust workspace | 167 unique tests PASS; fmt/check/clippy PASS |
| Native worker | 163 PASS, eight Unix-only skips |
| WSL worker | 171 PASS |
| Real uv/worker/Supervisor/inspection/notebook/async-package smokes | PASS |
| Windows/WSL source identity before/after gate | 114 files match |
| Real Windows pipe/Host/WSL package event proof | PASS, exit 0 |
| Independent notebook and worker-exit verification | PASS, exit 0 |

The credential-isolation test's nested subprocess result is not counted twice.
Platform-specific script skips remain unchanged. Source hashes describe the final
compiled source; documentation-only evidence/ledger edits follow verification.

Artifacts: [final combined gate](phase5-async-packages-20260921.gate.txt),
[real pipe and independent verification](phase5-async-packages-20260921.real.txt),
[registry regression before repair](phase5-async-packages-20260921.registry-before.txt),
[source hashes](phase5-async-packages-20260921.sources.json).

Reproduce with the existing source-identity Windows dispatcher, then the
[isolated Host preparation helper](phase5-host-core-20260921.prepare.py), current
Runtime build and Windows pipe example using a fresh Host state directory.
The real uv proof is part of check-rust.sh and cannot be skipped by a green native
Windows-only gate.

## Remaining work

Project create/register still execute uv synchronously. Long Session startup and
inspection can still occupy serial request admission. Those paths need responsive
control dispatch and acceptance evidence before P5-07 closes. Autostart, resource
metrics and full Host acceptance also remain; the full goal stays active.
