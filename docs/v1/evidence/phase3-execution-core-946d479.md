# Phase 3 execution-core verification evidence

Validated source revision: `946d4798dba0505a4d3415b84e070767e8c6e446`

WBS items covered:

- `P3-EXE-01` — persistent IPython namespace
- `P3-EXE-02` — stdout/stderr capture
- `P3-EXE-03` — final expression result capture
- `P3-EXE-04` — explicit rich MIME display capture
- `P3-EXE-05` — top-level async
- `P3-EXE-06` — structured Python exception outcome with Session survival

No GitHub Actions / GitHub-hosted CI was used.

## Environment

- Linux 6.18.44 x86_64
- Python 3.13.5
- IPython 9.14.0
- pytest 9.0.2
- Git 2.47.3

## Source identity

`worker/src/cokernel_worker/execution.py` was reconstructed from the GitHub blob for the validated revision and checked with local `git hash-object` before execution.

- GitHub blob SHA: `5320cfdca504f1495b5bf592cd2dc9a66cd60888`
- local `git hash-object`: `5320cfdca504f1495b5bf592cd2dc9a66cd60888`
- result: exact match

## Targeted execution proof

A local harness executed the exact `ExecutionEngine` source and asserted the following behaviors:

1. `x = 123`, then `x + 1` returns `124` in the same engine namespace.
2. stdout and stderr are captured independently (`hello-out` / `hello-err`).
3. `40 + 2` produces final MIME result `text/plain = "42"` without stdout duplication.
4. `display(HTML('<b>hello</b>'))` produces an explicit `text/html` rich display bundle.
5. top-level `await asyncio.sleep(0)` followed by `40 + 2` returns `42`.
6. `raise ValueError('boom')` returns a structured failed outcome with empty stdout/stderr, then a subsequent execution in the same engine succeeds, proving the persistent execution state was not destroyed by the user exception.

Result:

```text
PASS P3-EXE-01,P3-EXE-02,P3-EXE-03,P3-EXE-04,P3-EXE-05
PASS-EXTRA P3-EXE-06
```

In addition, the current execution test suite content was reconstructed and run against this exact production source:

```text
..................                                                       [100%]
18 passed in 0.61s
```

The suite includes persistent namespace/history, stdout/stderr, final expression handling, rich display/MIME normalization, top-level await, structured exceptions, hostile exception rendering containment, invalid rich-output containment, and reset semantics.

This evidence is sufficient to close the six execution-core WBS rows above at this revision. It does not replace the Rust/WSL Session Supervisor canonical Gate required for `P3-GATE`.
