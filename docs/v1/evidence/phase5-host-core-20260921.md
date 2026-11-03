# Phase 5 Host supervision component evidence — 2026-09-21

Base: `f35ca99711a8d6c102ef9e47b857e0e377f87cbb` plus the working tree. This
closes P5-01..P5-05 locally, not P5-GATE or any actual Desktop/MCP acceptance.
No commit, PR or GitHub issue closure was performed.

## Implemented and demonstrated

- The Windows Host binary runs independently of Desktop. Its bounded local WSL
  configuration produces explicit wsl.exe argv and an invisible child window.
  A real binary entrypoint smoke holds persisted STOPPED without launching Runtime.
- Host owns one persistent Runtime bridge, validates handshake, assigns/correlates
  UUID requests, handles reordered replies and separates events from responses.
  Dropped caller futures do not corrupt the connection or cancel accepted work.
- Desired state is locked per Host state directory, atomically replaced and
  reopened. Corrupt/oversized files fail explicitly. A failed state save cannot
  start Runtime. Host exit preserves desired state; explicit Stop persists STOPPED.
- Unexpected failure reconnects with a new generation and visible last_error.
  Retry delays/counts are bounded; incompatible protocol is terminal until explicit
  action. STOPPED never reconnects, including after a refused stop and later crash.
- Whole-Runtime shutdown stops supervised workers, settles accepted captures and
  flushes dirty documents. A save conflict refuses explicit shutdown while retaining
  document memory and the RPC connection. Host stop/exit preserves that connection
  for reconciliation. Nonzero/forced exits are not reported as clean shutdown.

The real Windows `wsl_host_smoke.exe` used the production Host library and actual
wsl.exe against an isolated development Ubuntu Runtime. It created a uv Project,
created/edited/executed a notebook, obtained 124 and safely read
secret_from_master=123456789 after dropping one caller and creating another.
It then stopped Runtime with a live worker and dirty metadata, restarted it,
reopened the saved file and verified the dirty edit was flushed. No live Session
was silently reconstructed. A separate /proc check confirmed the worker PID no
longer existed. These callers are Rust integration clients, not Desktop or MCP.

## Verification

| Check | Result |
|---|---|
| Native Windows workspace clippy/tests | PASS: 55 unique Rust tests before the final STOPPED recovery regression |
| Final native Host clippy/tests | PASS: 16 lifecycle process tests + Named Pipe feasibility test |
| WSL canonical scripts/v1/check.sh | PASS: 152 unique Rust tests, 167 worker tests, Project/worker/Supervisor/inspection/notebook smokes |
| Final WSL Host clippy/tests | PASS: 16 lifecycle process tests + Unix inherited-lock regression |
| Real Windows Host + WSL Runtime + worker | PASS: shared state, save, stop, restart, saved-file recovery and worker exit |
| Real Windows Host serve entrypoint | PASS: remains alive in STOPPED, generation 0, no Runtime launch |
| Final Windows/WSL source identity | PASS: 101 source/fixture/lock files match by SHA-256 |

The environment-isolation test runs itself once in a child process with seeded
test-only credential and WSLENV variables. Its nested one-test output is not an
additional unique test. Tests also cover oversized/malformed handshake, unknown
response ID, bounded oversized requests, stderr flooding, cancellation, timeout
without replay, shutdown rejection/failure, persisted stop/restart and circuit
breaking. Timeouts and test skips were not relaxed to obtain PASS.

The final Host-only follow-up gives the reader a bounded opportunity to consume
the final response when child exit races stdout delivery, and prohibits recovery
when desired state is STOPPED. Host tests and clippy were rerun on both platforms.
The final STOPPED-recovery change does not alter the real successful-stop path;
the last real-WSL smoke already uses acknowledged shutdown and refusal handling.
Runtime/worker code is unchanged from the complete Linux gate.

## Failures found and fixed

The first native mutation-count test compared platform newlines; it now compares
logical lines. A Linux parallel-process test exposed delayed state-lock release
when another process briefly retains an inherited descriptor. StateStore now
explicitly unlocks before acknowledging exit; a deterministic duplicated-descriptor
regression also proves immediate reacquisition.

Review found that merely checking the child exit code could lose dirty edits on
save failure. Shutdown now waits for a correlated success response; refusal keeps
the connection and document state. Both fake Host tests and the actual framed
Runtime notebook smoke verify refusal, retained unsaved metadata, preservation of
external file changes, explicit reconciliation and a subsequent successful stop.

## Reproduction and evidence boundaries

Toolchain: Windows Rust/Cargo 1.91.1, Python 3.11.9; WSL Rust/Cargo 1.88.0,
Python 3.12.14, uv 0.12.11, IPython 9.17.1. Linux tests use the native development
copy at /home/ymsaki/.cache/cokernel-v1-dev/repo and the same private tool/cache
paths recorded in Phase 4 evidence. The installed managed distro is not proven.

The [preparation helper](phase5-host-core-20260921.prepare.py) creates a unique
test Project root and Runtime wrapper under the developer cache, never altering
an existing Project or WSL configuration. After building Runtime in WSL and the
Host example natively, the real command was:

```powershell
target/debug/examples/wsl_host_smoke.exe target/dev-tools/host-wsl.json C:/programing/CoKernel/target/dev-tools/host-state-final
```

Run the [cleanup verifier](phase5-host-core-20260921.verify.py) under WSL and the
[entrypoint verifier](phase5-host-core-20260921.entrypoint.py) under native Python.
These helpers record machine-specific development paths, not installer behavior.

Logs: [Windows workspace](phase5-host-core-20260921.windows.txt),
[final Windows Host](phase5-host-core-20260921.host-windows.txt),
[Linux canonical gate](phase5-host-core-20260921.linux.txt),
[final Linux Host](phase5-host-core-20260921.host-linux.txt),
[real Host and cleanup](phase5-host-core-20260921.real-host.txt).
The [source manifest](phase5-host-core-20260921.sources.json) records final file
hashes; documentation/ledger-only updates follow verification.

Still required: current-user Named Pipe ACL/auth, real client reconnection,
Runtime event forwarding/initial snapshots, responsive async package/control
dispatch, autostart, metrics, Windows import streaming, and Phase 5 full acceptance.
Unexpected EOF has best-effort final flushing; durable recovery of unsaved edits
when storage is unavailable or Runtime crashes is not proven here. Force-killing
wsl.exe is not claimed as Linux descendant containment. Desktop, actual Codex/web
ChatGPT, MCP/tunnel security, installation and JAX GPU remain later goal work.
