# Phase 5 Runtime events and interrupt regression evidence — 2026-09-21

Base: `f35ca99711a8d6c102ef9e47b857e0e377f87cbb` plus the working tree.
This records component progress within **P5-07**, which remains IN_PROGRESS.
It does not close P5-GATE or Desktop/MCP/JAX GPU acceptance. No commit, PR or
issue closure was performed.

## Implemented and verified

- A Runtime-wide event hub provides ordered domain events with bounded payloads
  and queues. Heartbeats and private variable-inspection responses are excluded.
- A dedicated framed writer serializes complete RPC responses and event frames;
  an event cannot cancel a partially written response. Subscriber lag explicitly
  requests resynchronization and resets the subscription at a known watermark.
- `runtime.get_snapshot` returns bounded Project, Session and open-document
  summaries, totals and surrounding event watermarks. It is a live observation,
  not an atomic transaction across every domain object.
- Host forwards events with Runtime generation and sequence. Initial Named Pipe
  snapshots include Runtime state when available; unavailable state is explicit.
  Bounded client backpressure prevents a healthy reader from being disconnected
  merely because a short burst exceeds the immediate queue capacity.
- Execution completion events report the persistence outcome after the Runtime
  completion record has been updated. Clients can distinguish execution output
  from a successfully persisted notebook result.

## Interrupt regression and repair

The combined gate exposed `interrupt returned Failed, expected INTERRUPTED`.
A deterministic worker test then reproduced SIGINT escaping immediately after
the execution-start frame. The old worker announced start before installing its
interrupt guard; Supervisor also permitted signalling before worker acknowledgement.

Supervisor now retains an early interrupt request until the validated start event
and signals only once. The worker guards its start frame, handles interrupts at
capture setup/cleanup boundaries, restores stream/display hooks and ignores idle
SIGINT so a late signal cannot terminate the protocol loop. Execution restores
the normal interrupt handler within its guard. Notebook-installed custom signal
handlers are not a security or cancellation guarantee.

Tests cover delayed start acknowledgement, immediate start-frame interrupt, idle
SIGINT, capture-entry and guard-exit interruption, valid terminal output and the
same worker's surviving namespace. A fixture missing the real shell's execution
counter initially hung a socket test; the fixture was corrected and its read is
now bounded so future fixture exceptions fail promptly. No production timeout or
acceptance condition was weakened.

## Actual Windows / WSL proof

The updated `wsl_pipe_smoke` uses production Host code, Windows Named Pipes,
actual wsl.exe, a fresh uv Project and real IPython worker. An observer receives
the existing primary Session in its initial snapshot, then sees execution start,
output and `notebook.execution_completed` with `PERSISTED` and the correct
operation ID/Runtime generation. Completion is observed without polling.

The same proof also verifies chunked import from a Japanese Windows path,
accepted execution after the submitting connection closes, shared primary Session
memory, saved output 124, dirty metadata flush and reopen after explicit stop.
An independent WSL process validates the saved file with nbformat 5.11.1, checks
the attachment and metadata, confirms failed import left no file, and confirms
the worker PID no longer exists.

Final isolated proof root:
`/home/ymsaki/.cache/cokernel-v1-dev/host-proof-d1ac4da06eae`.
These are Rust clients exercising Desktop transport, not actual Desktop, Codex
or ChatGPT clients. The developer Ubuntu distro does not prove managed-distro
security, installation, secret isolation or JAX GPU acceptance.

## Verification

| Check | Result |
|---|---|
| Canonical Windows `check.ps1`, including actual WSL `check.sh` | PASS, exit 0 |
| Native Rust workspace | 76 unique tests PASS; fmt/check/clippy PASS |
| WSL Rust workspace | 159 unique tests PASS; fmt/check/clippy PASS |
| Native worker suite | 163 PASS, eight Unix-specific tests skipped |
| WSL worker suite | 171 PASS, including production signal-path tests |
| Real uv/worker/Supervisor/inspection/notebook smokes | PASS |
| Windows/WSL source identity before and after gate | 110 files match |
| Actual Windows pipe snapshot/live-event proof | PASS, exit 0 |
| Independent saved-file and worker-exit verification | PASS, exit 0 |

The credential-isolation lifecycle test invokes itself in a subprocess; that
nested one-test result is not counted as an additional unique Rust test. Script
tests have existing platform-specific skips. The final canonical log predates
the documentation-only ledger change from TODO to IN_PROGRESS.

## Artifacts and reproduction

- [Final canonical gate](phase5-events-20260921.gate.txt)
- [Actual pipe proof and independent verification](phase5-events-20260921.real.txt)
- [Deterministic failing interrupt test before repair](phase5-events-20260921.interrupt-before.txt)
- [SHA-256 source inventory](phase5-events-20260921.sources.json)

Use the [existing isolated-root helper](phase5-host-core-20260921.prepare.py),
build the current Runtime in the native Linux development copy, then run:

```powershell
cargo run -p cokernel-host --example wsl_pipe_smoke -- target/dev-tools/host-wsl.json C:/programing/CoKernel/target/dev-tools/events-final-state-proof
```

Use a fresh Host state directory for another run. Run
`scripts/v1/verify_host_pipe_smoke.py` inside WSL with nbformat 5.11.1 and the
captured output-log/root-file arguments. Canonical validation uses the established
source-identity-checking Windows dispatcher and actual Linux gate; no checks are
replaced by log inspection.

## Remaining work

Runtime request dispatch still waits for synchronous uv/package operations.
P5-07 requires asynchronous operation IDs/progress and responsive control during
that work. Event delivery alone does not satisfy this criterion. Autostart,
resource metrics and full Host acceptance remain open; the delivery goal stays active.
