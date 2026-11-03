# Phase 4 notebook document component evidence — 2026-09-21

Base: `f35ca99711a8d6c102ef9e47b857e0e377f87cbb` plus the Phase 3 repairs and
the current Notebook Document Service changes. This closes document components,
not the complete Phase 4 gate or the Desktop/MCP/JAX delivery goal.

## Scope and source identity

Implemented `crates/cokernel-runtime/src/notebook/{model,storage,service}.rs`.
Windows source is `C:\programing\CoKernel`; Linux validation runs in the native
Ubuntu snapshot `/home/ymsaki/.cache/cokernel-v1-dev/repo` with the development
environment recorded in the Phase 3 evidence. After the successful run, all
180 snapshot inventory files matched the Windows working tree by SHA-256.
[Source manifest](phase4-documents-20260921.sources.json) binds the tested code;
subsequent evidence/ledger/planning edits are excluded.

## Verified behavior

- nbformat 4.0–4.5 structural model; preserve metadata, attachments, kernelspec,
  source representation, raw/Markdown/code and stream/error/rich MIME results.
- Normalize legacy missing IDs without rewriting disk on open; retain valid IDs.
- Reject malformed JSON, duplicate object keys, malformed shapes, invalid or
  duplicate cell IDs, unsupported versions/types and byte/cell/edit bounds.
- A 2 MiB output round-trips without loss; the complete compatibility fixture
  round-trips with equality of parsed JSON values.
- Create/open/deduplicate/list notebooks within a Project; collision rejection.
- Atomic edit batches, monotonic revisions and stale-caller conflict rejection.
- Separate in-memory and saved content hashes, dirty status and explicit reload.
- Detect external replacement, including identical bytes at a new inode; retain
  unsaved edits and external file bytes instead of overwriting on a known conflict.
- Descriptor-relative Linux file operations reject traversal, Windows-style paths,
  parent/leaf symlinks, hard-linked files and FIFO inputs without blocking.
- Injected partial write leaves the original intact and removes temporary files.
- Injected I/O failure leaves edits recoverable as SAVE_FAILED; retry saves them.
- Injected directory-sync failure after actual replacement is reported as
  SAVE_FAILED; retry uses the newly installed file identity and succeeds.
- Project ID checks reject another Project's notebook ID. Symlink replacement
  cannot cause saving through a link into another Project.

The metadata-preservation test does not launch Python and cannot establish the
target-Project execution requirement. That remains integration acceptance.

## Commands and results

| Verification | Result |
|---|---|
| Windows `cargo fmt --all` / Linux `cargo fmt --all --check` | PASS |
| Windows and Linux `cargo clippy --workspace --all-targets -- -D warnings` | PASS |
| Windows `cargo test --workspace` | 36 PASS; Linux persistence tests are cfg-excluded |
| Linux `cargo test --workspace` | 115 PASS; no ignored/failed tests |
| New model tests | 7 PASS on Windows and Linux |
| New Linux storage unit tests | 3 PASS |
| New Linux document failure unit tests | 2 PASS |
| New Linux document integration tests | 6 PASS |
| Independent `nbformat.validate` of compatibility.ipynb | PASS, nbformat 5.11.1 / Python 3.12.14 |

Linux total: domain 2, MCP scaffolding 3, protocol 4, Runtime unit 72, notebook
model 7, notebook service 6 and existing Session process tests 21. The existing
Session tests were not weakened. Full output:
[Linux Rust verification](phase4-documents-20260921.linux-output.txt).

The independent standard-format check used an ephemeral development environment:

```powershell
uv run --no-project --python 3.12 --with nbformat python -c 'import pathlib, nbformat; p=pathlib.Path("tests/fixtures/notebooks/compatibility.ipynb"); n=nbformat.read(p, as_version=4); nbformat.validate(n); print("nbformat", nbformat.__version__, "compatibility fixture: PASS")'
```

It does not add nbformat or Jupyter server to product/Project dependencies.

## Failed attempts and scope limits

- A malformed-fixture test initially used an unescaped JSON-pointer slash for a
  MIME key. Correcting it to `image~1png` made the intended invalid input reach
  the parser and fail as expected.
- Windows's newer clippy required `slice::from_ref` in one test. Linux clippy also
  caught a helper after the test module. Both were fixed and affected checks rerun.
- Filesystem saves are optimistic against external writers; a hostile concurrent
  writer in the last check-to-rename interval is outside this synchronization
  contract. Product-mediated mutation is serialized. See PROJECT_NOTEBOOK_MODEL
  section 21 for the boundary, limits and explicit reconciliation behavior.
- This is the Runtime domain API. Public Runtime/Host/MCP routing, output
  application and authoritative execute-cell, chunked import, close/autosave
  orchestration and complete Phase 4 acceptance are still outstanding.
- Phase 3 real-worker canonical smoke evidence remains separate; this run tests
  the Rust workspace including fake-worker process regressions, not a new full
  Windows→WSL canonical gate. Phase 4 requires its own integrated final gate.

Next: P4-07/P4-08 output and cell execution integration, P4-09/P4-10 bounded
import transactions, then P4-12/P4-A-01/P4-GATE with real Project/Session proof.
