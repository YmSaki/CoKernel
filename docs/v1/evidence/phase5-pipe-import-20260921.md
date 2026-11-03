# Phase 5 current-user IPC and Windows import evidence — 2026-09-21

Base: `f35ca99711a8d6c102ef9e47b857e0e377f87cbb` plus the working tree.
This closes **P5-06 and P5-10** locally. It does not close P5-07, P5-GATE,
Desktop/MCP acceptance or the overall goal. No commit, PR or issue closure.

## Implemented boundary

- Host binds a per-user Named Pipe with a protected current-user DACL, rejects
  remote clients and verifies the connected peer SID after reading hello. The
  sequential client verifies the server process SID. Tests inspect the actual DACL,
  prove kernel access denial and verify thread impersonation was reverted.
- Host exposes protocol-1 hello, status/version/control, permitted domain RPC and
  automatic Host status snapshots/events. Each client has bounded framing and
  queues; duplicate IDs and malformed frames affect only that connection.
- Distinct clients can reuse their own request IDs without cross-delivery.
  The ninth pending request is rejected while all eight accepted requests finish.
  Cancelled client reads/RPCs require reconnect; accepted domain work continues.
- Windows import opens the selected local regular file once, rejects unsafe path
  classes and oversized/empty input, excludes concurrent write/delete sharing,
  hashes the same handle and streams at most 256 KiB chunks. Runtime owns format
  validation, target path checks, transaction cancellation and atomic installation.
  The selected Windows path never enters the Runtime request. No drive mount or
  /mnt path translation is used by the import implementation.

## Actual Windows / WSL proof

`wsl_pipe_smoke` uses real Windows Named Pipes and the production Host library,
actual wsl.exe, a fresh development Ubuntu Project and uv/IPython worker. It:

1. Writes a notebook at a Japanese Windows path, larger than one chunk, with
   metadata and a markdown attachment; imports it via `notebooks.import_file`.
2. Confirms metadata/attachment survive and that collision and invalid notebook
   imports are rejected without leaving an invalid target.
3. Connects two independent Rust pipe clients; both receive the same primary
   Session ID. One executes and disconnects while Python is still running.
4. The other sees persisted output 124 and reads `secret_from_master=123456789`.
   Closing every client and reconnecting preserves the Host generation.
5. Adds dirty metadata, stops Runtime through the pipe, restarts it and confirms
   saved edits survive while volatile Sessions are not silently reconstructed.
6. An independent WSL verifier checks the worker PID is gone, saved notebook
   validates with nbformat 5.11.1, output/attachment/edit persist, and the invalid
   import target is absent.

These are Rust clients exercising the future Desktop transport, not actual Human
Desktop UI, Codex or ChatGPT MCP clients. The development distro still has its
existing configuration; this does not prove managed-distro automount/interop,
identity separation, secrets, installation or JAX GPU acceptance.

## Verification

| Check | Result |
|---|---|
| Windows canonical `scripts/v1/check.ps1` including actual WSL gate | PASS, exit 0 |
| Native Rust workspace | 71 unique tests PASS, check/fmt/clippy PASS |
| Native Host subset | 7 library + 16 lifecycle + 9 pipe tests PASS |
| WSL Rust workspace | 153 unique tests PASS, check/fmt/clippy PASS |
| WSL worker suite | 167 tests PASS, worker protocol/Supervisor/inspection/notebook smokes PASS |
| Native worker suite | 163 PASS, four Unix-only tests skipped; corresponding production path exercised in WSL |
| Windows/WSL source identity | 108 source/fixture/lock/script files match before and after gate |
| Final real pipe/import proof | PASS, exit 0 |
| Independent saved-file/worker-exit verification | PASS, exit 0 |
| Real Host service entrypoint in STOPPED | PASS; stays alive, no Runtime launch |

The lifecycle credential-isolation test runs itself once in a subprocess; that
nested one-test result is not an extra unique test. Existing platform-specific
script checks also skip Windows-only subprocess checks on Linux. No test/timeout
was relaxed to obtain PASS. Initial development found a pipe-instance rotation
race in a raw test client; the test now uses bounded PIPE_BUSY retry like the
real client. Current source compiles on both native Windows and Linux.

## Reproduction and artifacts

Toolchains and native Linux development copy match the preceding
[Host core evidence](phase5-host-core-20260921.md). The existing
[preparation helper](phase5-host-core-20260921.prepare.py) creates a fresh test root
and trusted Runtime wrapper; it does not change existing user Projects or WSL
configuration. After canonical Windows/WSL verification, run:

```powershell
cargo run -p cokernel-host --example wsl_pipe_smoke -- target/dev-tools/host-wsl.json C:/programing/CoKernel/target/dev-tools/pipe-final-state-proof
```

Capture the output, then run `scripts/v1/verify_host_pipe_smoke.py` inside WSL with
nbformat 5.11.1 and the output-log/root-file arguments. The final preparation root
was `/home/ymsaki/.cache/cokernel-v1-dev/host-proof-2cc5000119b9`.

Artifacts: [combined canonical gate](phase5-pipe-import-20260921.gate.txt),
[real pipe/import and independent verification](phase5-pipe-import-20260921.real.txt),
[source hashes](phase5-pipe-import-20260921.sources.json).
The gate uses the existing source-identity-checking
[Windows dispatcher](phase4-runtime-20260921.runner.ps1) pattern; final identity
inventory contains the new pipe/import sources. Documentation-only changes follow
the compiled-source verification.

## Remaining Phase 5 work

P5-07 still needs full Runtime event forwarding, initial/reconnect domain snapshots,
explicit lag recovery and responsive control during asynchronous package work.
The implemented `host.snapshot` includes Host status only. Package RPC is still
synchronous inside Runtime; multiplexing Windows clients alone does not fix that.
P5-08 autostart and P5-09 resource metrics also remain. Full Phase 5 acceptance
waits for those requirements; this evidence does not promote them to DONE.
