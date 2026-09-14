# Phase 3 worker component batch evidence

Status: PASS for the selected components below; NOT a Phase 3 Gate PASS.

Source baseline: `946d4798dba0505a4d3415b84e070767e8c6e446`.
Concurrent parent `4db31b40cbcc2e97c2f6fe6c49ec62c2b4ae378f` adds only two evidence documents; both are preserved. No production/test source changed between those revisions.

## Environment and provenance

Linux 6.18.44 x86_64; Python 3.13.5; IPython 9.14.0; pytest 9.0.2.
This is a source-faithful component snapshot retrieved through the GitHub connector, not a full git checkout or uv-managed installation. Before editing, all nine fetched files below matched GitHub's blob IDs using local `git hash-object`. No reimplementation of the tested production logic was used.

| File | Baseline Git blob |
|---|---|
| worker/src/cokernel_worker/__init__.py | e678ccac5db2ee72eb06e31768cd85c61bc3e6b5 |
| worker/src/cokernel_worker/execution.py | 5320cfdca504f1495b5bf592cd2dc9a66cd60888 |
| worker/src/cokernel_worker/inspection.py | 886b7aecd375354dd1edda24b85f66c831eaf58a |
| worker/tests/test_execution.py | b439ca10c9d206acfb9bdbb9252fb6bda592bd61 |
| worker/tests/test_inspection.py | 58f52703837c5533748884d32c90d36d865c0624 |
| worker/tests/test_inspection_name_bound.py | e6407e91e9eaa6832e8a2a25e324b672ccdf453d |
| worker/tests/test_inspection_support_contract.py | ef37c17aa6de5aeebd841a458bfcd2b1eae1cd71 |
| worker/tests/test_stream_capture_prefix.py | 78f65f98827d86237c4b26a1ca1cddcd9ccf706d |
| worker/tests/test_stream_limits.py | 1fde4363e9fe9286891b61448806b2a5861eddc6 |

Tested changes:

- `inspection.py`: `f878a8e5189d6d87464a5f3a3df3a4b8b0e360c3`.
- New `test_inspection_boundary_regressions.py`: `7c65625441efe512e43342a20a51d2b855764d3d`.
- All other tested production/test files retain the baseline hashes above.

## Reproduced failures and fixes

The new 53-case regression file against the original inspection source produced **34 FAIL / 19 PASS**, exit 1. Against the corrected source all 53 pass.

1. Tuple membership in type dispatch invoked custom metaclass equality. The 24 direct/nested get/list cases cover raising, true-returning and false-returning comparisons. Identity-only checks replace equality/hash-based dispatch. The corpus also tests 16 object/list/dict/str-subclass combinations with hostile repr/str/iter/getitem/bool/getattr/getattribute/comparison/operator hooks.
2. Inspection response budgeting did not match the result actually sent by WorkerLoop. Supported get results omitted `reason: null` from accounting; unsupported get results skipped budgeting entirely; list results omitted the `variables` wrapper. All result fields now count toward the same UTF-8 JSON result budget. Exact-size acceptance and one-byte-under rejection cover six get-result cases and four list-result cases. Three depth-boundary cases supplement existing item/string/namespace limit tests.

The limit applies to the method result object. The outer worker frame remains subject to its separate transport frame limit. Existing inspection behavior stays fail-closed; no MCP annotation or safety permission is relaxed.

## Executed commands

From the snapshot root, using the listed source files and installed system dependencies:

```sh
PYTHONPATH=worker/src PYTEST_DISABLE_PLUGIN_AUTOLOAD=1 python3 -m pytest -q \
  worker/tests/test_execution.py \
  worker/tests/test_inspection.py \
  worker/tests/test_inspection_name_bound.py \
  worker/tests/test_inspection_support_contract.py \
  worker/tests/test_stream_capture_prefix.py \
  worker/tests/test_stream_limits.py \
  worker/tests/test_inspection_boundary_regressions.py
```

Result: **95 PASS**, exit 0 (42 existing + 53 new cases). No selected test was skipped.

Additional local checks:

```sh
python3 -m unittest discover -s scripts/v1 -p test_work_status.py -q
python3 scripts/v1/validate_work_status.py --check
python3 -m compileall -q worker/src worker/tests scripts/v1
```

Results: **11/11 unittest PASS**; **201 unique WBS rows and all summary counts agree**; compileall exit 0. The ledger checker verifies structure/arithmetic only, never runtime correctness or evidence sufficiency.

## WBS closure mapping

| Item | Evidence in the selected tests |
|---|---|
| P3-EXE-01 | test_namespace_persists_across_cells: x=123 followed by x+1 yields 124 in one ExecutionEngine |
| P3-EXE-03 | final-expression MIME, no duplicated stdout, one formatter invocation, history and semicolon tests |
| P3-EXE-05 | test_top_level_await_uses_ipython_semantics |
| P3-INSP-01 | list metadata/filter/support-probe tests in test_inspection.py and test_inspection_support_contract.py |
| P3-INSP-02 | exact built-in value, integer bounds, invalid identifier and unsupported-value tests |
| P3-INSP-03 | existing hostile corpus plus 24 metaclass and 16 object/subclass regression cases; no hooks invoked |
| P3-INSP-04 | full get/list result byte-budget boundaries, depth, namespace, identifier, item and string limit tests |

These seven existing component criteria are DONE. P3-EXE-02/04/06/07/08 have useful component evidence, but remain READY_VERIFY because their broader output/transport/process criteria are not fully proven by this batch. The prior P3-V-06 fixture receipt is reconciled separately; its fixtures were not rerun here.

## Explicit exclusions and blockers

NOT executed: full worker pytest suite, uv sync/overlay launch, worker protocol/output-limit suites, Rust fmt/check/clippy/test, check.sh/check-rust.sh/check.ps1, Runtime Session Supervisor/inspection smokes, or Windows/WSL/GPU/MCP/tunnel acceptance. No real-process Session concurrency/crash proof is claimed.

The executor lacks cargo/rustc/rustfmt and a Windows/WSL target. DNS resolution for github.com, raw.githubusercontent.com, pypi.org and static.rust-lang.org failed. GitHub connector reads/writes remained available.

P3-V-05 stays TODO; environment-dependent verification, canonical acceptance and P3-GATE stay BLOCKED. Issues #29/#39/#40 remain open. No GitHub Actions / GitHub-hosted CI was used, triggered, monitored or waived into a PASS.
