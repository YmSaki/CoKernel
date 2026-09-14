# Phase 3 safe-inspection verification evidence

Validated source revision: `946d4798dba0505a4d3415b84e070767e8c6e446`

WBS items covered:

- `P3-INSP-01` — `list_variables`
- `P3-INSP-02` — `get_variable`
- `P3-INSP-03` — hostile/custom object behavior is not invoked
- `P3-INSP-04` — inspection depth/item/string/response bounds

No GitHub Actions / GitHub-hosted CI was used.

## Environment

- Linux 6.18.44 x86_64
- Python 3.13.5
- Git 2.47.3

## Source identity

`worker/src/cokernel_worker/inspection.py` was reconstructed from the GitHub blob for the validated revision and checked with local `git hash-object` before execution.

- GitHub blob SHA: `886b7aecd375354dd1edda24b85f66c831eaf58a`
- local `git hash-object`: `886b7aecd375354dd1edda24b85f66c831eaf58a`
- result: exact match

## Targeted inspection proof

The exact production inspection implementation was exercised directly:

1. `list_variables` returned sorted safe/coarse metadata for supported built-in values.
2. `get_variable` returned an exact bounded nested built-in value.
3. A hostile custom object implementing `__repr__`, `__str__`, `__iter__`, `__getattr__`, and `__bool__` remained unsupported and **none of those hooks executed** (`calls == 0`).
4. Explicit low bounds rejected overlong strings, excessive nesting/item budgets, and oversized namespaces with `InspectionError`.

Result:

```text
inspection blob: 886b7aecd375354dd1edda24b85f66c831eaf58a
PASS P3-INSP-01,P3-INSP-02,P3-INSP-03,P3-INSP-04
```

This closes the evidence requirement for the four safe-inspection implementation rows at this revision. The real-worker `P3-V-10` smoke remains dependent on the canonical Rust/WSL Gate and is not claimed complete here.
