# Phase 3 production live-output evidence

Date: 2026-09-15
Base revision: `174ef984c353900a1690e0e082b2654a5c1e3069`
Implementation revision: `37294b33878f56a9b45197f761d5cbcd91211f8a`
Contract: [OUTPUT_STREAMING.md](../OUTPUT_STREAMING.md)

## Gap and implementation

The baseline production worker buffered output until `run_cell` returned, then
replayed all stdout, all stderr, and all displays in separate groups. This lost
cross-output ordering, hid progress during long-running cells, lost buffered
output on abrupt exit, and retained every rich payload until cell completion.

The production worker now supplies a synchronous output sink to the same
persistent IPython engine. The change delivers ordered stream/display/result
frames during execution, applies existing wire budgets immediately, chunks
stream encoding, preserves a strict truncated prefix, seals saved output
references before terminal frames, and defers SIGINT across output-frame and
sequence commit. Rich payload construction no longer invokes `asdict` deepcopy
hooks. No new execution service, wire event name, or dependency is introduced.

This is production implementation plus tests, not fake-worker fixture coverage.
The buffered no-sink engine convenience API remains available for direct tests.

## Source identity

All six baseline production files and three baseline test files were retrieved
at the base revision and matched by local Git blob hash. The changed files below
were executed locally; GitHub create_blob responses matched their local hashes
before the branch was advanced.

| File | Tested/published Git blob |
|---|---|
| worker/src/cokernel_worker/execution.py | d3b4e74ee1daae4dd9453cb2a9f008f55e9573a7 |
| worker/src/cokernel_worker/protocol.py | 8e786385dc6a612df237a76db0f403cbbdd3a91d |
| worker/src/cokernel_worker/streaming.py | 45e6718616fa21e85e5f36895595df8a6c5206d9 |
| worker/tests/test_output_limits.py | d2add9b584f916867c8502edeb0a63ce1dd20fbf |
| worker/tests/test_protocol_contract.py | 8193e88c38a9d55e210f658509f114f8a654a7d1 |
| worker/tests/test_streaming_output.py | 7adb210694dbd9f39ef9a856b5cf0809f8b91a18 |

Unchanged execution tests: `b439ca10c9d206acfb9bdbb9252fb6bda592bd61`.
Unchanged production output budget: `261aa5a197966aa7a262567d2e303f7240732cf9`.
Unchanged production inspection: `f878a8e5189d6d87464a5f3a3df3a4b8b0e360c3`.

## Executed verification

Environment: Linux 6.18.44 x86_64, Python 3.13.5, IPython 9.14.0, pytest 9.0.2.
This is a source-faithful local snapshot, NOT a complete repository checkout or
uv-managed Project installation. No GitHub-hosted execution was used.

Explicit test selection:

```sh
PYTHONPATH=worker/src PYTEST_DISABLE_PLUGIN_AUTOLOAD=1 python3 -m pytest \
  worker/tests/test_execution.py \
  worker/tests/test_output_limits.py \
  worker/tests/test_protocol_contract.py \
  worker/tests/test_streaming_output.py -q --junitxml=/tmp/streaming-results.xml
python3 -m compileall -q worker/src worker/tests
git diff --check
```

Results: **64 passed**, 0 failures, 0 errors, 0 skips. Repeated final runs passed
in 4.49 s and 4.38 s respectively. Compileall and diff whitespace check passed.
The module counts are 18 existing execution tests, 10 output-budget tests,
6 protocol-contract cases, and **30 new streaming regression cases**.

The two updated existing modules now verify live-sink/chunked behavior rather
than assuming buffered replay. In particular the 7,000,001-byte stdout and stderr
floods each retain the exact 4 MiB source prefix and report the exact omitted-byte
total in matching finish/response accounting.

The new cases cover mixed output order, live flush, execution count, rich MIME
normalization, malformed output survival, retained-payload bounds, Unicode and
escaped stream floods, zero-byte capture, no-resume truncation, no deepcopy hook,
sealed saved references, IPython history/await/semicolon/errors, explicit unsupported
clear/update behavior, joined-thread output, and SIGINT during frame commit.

### Three actual worker-process proofs

Each test starts the real `python -m cokernel_worker` source entrypoint as a new
process, connects through a temporary AF_UNIX listener, checks ready PID and
handshake, uses finite deadlines, and reaps the process on exit/failure.

1. **Pre-completion delivery:** user code prints progress then waits for a file
   the parent creates only after receiving stdout. Output arrives before release;
   the same worker finishes with 42, exposes marker=41 through get_variable, and
   shuts down with exit 0.
2. **SIGINT survival:** after receiving live output the parent sends OS SIGINT.
   The worker emits the complete failed-operation lifecycle, then evaluates
   marker+1 as 42 in the same namespace/process.
3. **Crash output preservation:** code prints and flushes before `os._exit(23)`.
   The parent receives `before-crash\n` before EOF and observes exit 23. No false
   execution_finished is manufactured for the abruptly killed worker.

These are Python worker-process proofs, NOT Rust SessionSupervisor proofs.

### Red-before-green check

The baseline source snapshot was run against these two new regressions:
`test_production_preserves_mixed_output_order_and_execution_count` and
`test_rich_display_flood_does_not_retain_user_payloads_until_cell_end`.

Baseline: **2 failed**. Output order was grouped by type; the 200-display weakref
probe found **200** live payloads at cell end. Updated source: both pass; the
same probe retains at most **1** payload (the user's last local variable).
This is a retained-object proof, not a measured peak-RSS or process-wide memory
ceiling claim.

## Remaining scope and gate status

This supplements worker-side P3-EXE-02/04/07/08 evidence. It does not change WBS
DONE counts or close #29/#40. The prior 137-test full-worker result was NOT rerun
here; only the explicitly selected 64 cases above are claimed for this change.

Still unexecuted: complete worker pytest under uv, protocol fixture validation
in this run, cargo fmt/check/clippy/test, check.sh/check.ps1, and real Rust
SessionSupervisor/inspection smoke or Windows/WSL acceptance. This executor has
no cargo/rustc/rustfmt/PowerShell/wsl.exe; GitHub clone and Rust host lookup failed
DNS, and the CoKernel connector execution-target probe returned Resource not
found. No claim is made that the user's workstation itself is disconnected.

Display clear/update remains explicitly unsupported until a versioned wire,
Rust consumer, and Notebook output-mutation contract are implemented. Native
FD capture, unrelated background-task ownership, single-payload transient
allocations and stalled-receiver escalation are not solved by this change.
Phase 4 Notebook Document Service and subsequent product phases are not claimed
implemented by this worker slice.

## Publication note

An erroneous Contents API call created an empty OUTPUT_STREAMING.md in commit
`e2c2faf077a24d1614b33d8255c2c56ece71b31b`. The implementation commit replaces it
with the complete tested contract; history was preserved without force updates.
No existing source file was removed by that call. The unused candidate commit
`c42c399` was never made the branch head.

Only static workflow definitions were read to establish that cokernel-v1 pushes
are excluded and the v1 workflow is manual-only. No Actions run, status, job,
artifact or log was queried, triggered, or used for validation.
