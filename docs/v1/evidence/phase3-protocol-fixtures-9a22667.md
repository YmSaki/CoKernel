# Phase 3 protocol fixture validation evidence

WBS item: `P3-V-06`

Validated source revision: `9a226672084e51333ec5db3d449792899b31073f`

Validation path: repository-local `scripts/v1/validate_protocol_fixtures.py`; no GitHub Actions / GitHub-hosted CI.

## Environment

- Linux 6.18.44 x86_64
- Python 3.13.5
- Git 2.47.3

## Source identity

The validator and every fixture were reconstructed from the `cokernel-v1` GitHub blobs and checked with local `git hash-object` before execution.

| Path | GitHub / local blob SHA |
|---|---|
| `scripts/v1/validate_protocol_fixtures.py` | `ac71c6b777503b967d92c4e81ea8bd797010b7db` |
| `protocol/fixtures/bridge-hello.request.json` | `1ca27ede5d268cd83ea5540d3d41f4fed6d1c32c` |
| `protocol/fixtures/error.response.json` | `f0a7b8e9c66944afacdd1af5eb00bf86ffc37fbf` |
| `protocol/fixtures/runtime-status.response.json` | `8add018e52d45dbdf145e96bc5ddaf2f0b7e768e` |
| `protocol/fixtures/session-execute.request.json` | `6ddf9bf5a64f5958a9d19966b4da3f97b64831cb` |
| `protocol/fixtures/session-output.event.json` | `4afde9d6bd9ba7507ae1f6aee464816cfe6adfec` |

## Result

Command-equivalent execution:

```text
python3 scripts/v1/validate_protocol_fixtures.py
```

Result: **PASS — 5 / 5 fixtures**.

```text
OK protocol/fixtures/bridge-hello.request.json
OK protocol/fixtures/error.response.json
OK protocol/fixtures/runtime-status.response.json
OK protocol/fixtures/session-execute.request.json
OK protocol/fixtures/session-output.event.json
```

This closes the evidence requirement for `P3-V-06` at the validated revision. Phase 3 remains open: the Rust-equipped local/WSL gate (`cargo fmt/check/clippy/test`, Runtime/Session real-process smokes, and canonical acceptance proofs) is still required before `P3-GATE`.
