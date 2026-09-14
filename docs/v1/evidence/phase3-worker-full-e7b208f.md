# Phase 3 full worker pytest + source subprocess evidence

Validated branch base: `e7b208f4817509bb5d3237f3c9657eae8bc7d1b9` plus the two test/smoke additions in this evidence commit.

Scope: closes the worker/execution component rows explicitly mapped below and `P3-V-05`. This is **not** the canonical `scripts/v1/check-worker.sh` / `scripts/v1/check.sh` Gate and does not close Rust/WSL Session Supervisor acceptance.

## Environment

- Linux 6.18.44 x86_64
- Python 3.13.5
- IPython 9.14.0
- pytest 9.0.2
- uv 0.10.0
- no `cargo`, `rustc`, or `rustfmt`
- external package hosts unavailable by DNS from this executor

## Source identity

The complete production modules exercised by the worker suite were reconstructed from GitHub connector content and checked with `git hash-object` before execution:

| Path | Git blob |
|---|---|
| `worker/src/cokernel_worker/__init__.py` | `e678ccac5db2ee72eb06e31768cd85c61bc3e6b5` |
| `worker/src/cokernel_worker/__main__.py` | `0a86407780fd97094c8007e8d9932e88225af177` |
| `worker/src/cokernel_worker/execution.py` | `5320cfdca504f1495b5bf592cd2dc9a66cd60888` |
| `worker/src/cokernel_worker/inspection.py` | `f878a8e5189d6d87464a5f3a3df3a4b8b0e360c3` |
| `worker/src/cokernel_worker/output_limits.py` | `261aa5a197966aa7a262567d2e303f7240732cf9` |
| `worker/src/cokernel_worker/protocol.py` | `9bb154a1e3646dab5f9d450100a6dfbaedba51be` |
| `worker/pyproject.toml` | `137e910864a726e36a72d4747cff47b7e0f9c47e` |

All ten pre-existing worker test files likewise matched their GitHub blobs. This run adds:

- `worker/tests/test_protocol_contract.py` — local tested blob `d30af4d106648f911fa6d0b9de120c314dc59e0e`;
- `scripts/v1/smoke_worker_source.py` — local tested blob `a423193f2071e958bf8b261d25df5d6860ae453d`.

## Full worker pytest

Command:

```sh
PYTHONPATH=worker/src PYTEST_DISABLE_PLUGIN_AUTOLOAD=1 \
  python3 -m pytest worker/tests -q --junitxml=/tmp/worker-full.xml
```

Result from JUnit XML:

```text
tests=137 failures=0 errors=0 skipped=0
```

Result: **PASS — 137 / 137**.

The six new protocol-contract cases add explicit coverage for:

- outbound frame max-size enforcement before socket write;
- oversized inbound declared length rejection before payload read;
- unsupported protocol version fail-closed + worker survival;
- non-request frame type fail-closed + worker survival;
- reset semantics without worker replacement;
- 7 MB stdout + 7 MB stderr becoming bounded ordered worker output events with matching `output_count` and truncation accounting.

## Real AF_UNIX source subprocess smoke

Command:

```sh
PYTHONPATH=worker/src python3 scripts/v1/smoke_worker_source.py
```

Result:

```text
[worker-source-smoke] PASS
```

This launches the exact checked source as a separate `python -m cokernel_worker` process and connects through a temporary Supervisor-owned `AF_UNIX` socket. It verifies `ready`, PID correlation, handshake/capabilities, execute, `get_variable`, structured user exception failure with same-worker namespace survival, reset, missing-variable rejection after reset, shutdown, and zero worker exit status.

Version entry point also passed:

```text
PYTHONPATH=worker/src python3 -m cokernel_worker --version
1.0.0a0
```

## uv canonical-path attempt

The exact project file was present and `uv` is installed. The offline attempt was:

```sh
uv sync --project worker --dev --offline
```

It failed before executing project code because `ipython>=9,<10` is not present in the uv package cache. External package hosts are unreachable from this executor. Therefore this receipt does **not** claim `scripts/v1/check-worker.sh` or `P3-V-07`; only `P3-V-05` (the complete worker pytest suite on exact source) is closed.

## WBS closure mapping

| WBS | Evidence |
|---|---|
| `P3-WRK-01` | real separate worker process over Supervisor-owned AF_UNIX path; framed execute/get/shutdown PASS |
| `P3-WRK-02` | full tests cover ready/heartbeat/ping/reset/shutdown; source subprocess covers ready/reset/shutdown |
| `P3-WRK-03` | frame size, UTF-8/JSON, version/type/session/id/method/payload rejection and survival tests PASS |
| `P3-WRK-04` | serde_json integer boundaries, non-string object-key rejection, non-finite JSON rejection PASS |
| `P3-WRK-05` | bounded exception-name unit test + nameless exception serialized as non-empty `Exception`, worker survives |
| `P3-EXE-02` | real WorkerLoop emits bounded stdout/stderr events with ordered sequence and matching lifecycle count |
| `P3-EXE-04` | rich HTML/binary/SVG normalization and blob-limit worker tests PASS |
| `P3-EXE-06` | ValueError/SyntaxError/nameless errors become structured FAILED outcomes and worker/session remains usable |
| `P3-EXE-07` | event/operation/blob/stream bounds + omitted-byte/reason propagation + final-response agreement PASS |
| `P3-EXE-08` | malformed rich normalization and invalid transport JSON produce FAILED lifecycle without losing persistent worker state |
| `P3-V-05` | complete `worker/tests` suite: 137/137 PASS |

## Additional consistency checks

```sh
python3 scripts/v1/validate_work_status.py --check
python3 -m unittest discover -s scripts/v1 -p test_work_status.py -q
python3 -m compileall -q worker/src worker/tests scripts/v1
```

Results: WBS **201 rows / counts consistent**, ledger tests **11/11 PASS**, compileall **PASS**.

## Remaining blockers

`P3-V-01..04` still require Rust tooling; `P3-V-07..10` require the canonical uv/Rust/WSL/Windows paths; `P3-A-01..04` require real Session Supervisor acceptance. No Phase 4 work is unblocked. GitHub Actions / GitHub-hosted CI were not used, started, or monitored.
