# Phase 5 resource metrics and local gate audit — 2026-09-22

Base: `f35ca99711a8d6c102ef9e47b857e0e377f87cbb` plus working-tree changes.
P5-09, P5-11, P5-A-01 and P5-GATE are locally DONE on the evidence below.
This opens Phase 6 work, not release acceptance. No PR, commit, GitHub issue
closure, Windows sign-out or real autostart registration was performed.

## Resource implementation

`cokernel-domain::resources` owns typed observations; `cokernel-metrics` owns
Windows/Linux OS sampling and a bounded NVIDIA diagnostic. Host and Runtime use
one lazily created sampler each, with cached reads, one collection in flight,
roughly two-second observed cadence and ten-second background cadence. Slow OS
collection does not borrow the Runtime request owner or execute notebook code.
Monotonic sample age includes collection time; stale/stopped/warming observations
are explicit. CPU is a counter delta, not lifetime average.

Host `resources.get_snapshot` combines separate Windows/Runtime snapshots and
labels missing Linux data rather than failing the whole response or starting WSL.
The Linux query has a 750 ms deadline; generation/state changes discard its reply.
No client parameter can select a file, executable, command or process to inspect.
Values retain units, scope and availability. Linux MemAvailable, Windows physical
availability, cgroup v2 current/ancestor limits and filesystem capacity are distinct.
GPU UUID/utilization/VRAM/temperature/power preserve N/A; global usage is never
assigned to a Session. The Windows CPU path declines partial processor-group data.

NVIDIA runs from a protected system directory with fixed query arguments, closed
stdin, discarded stderr, minimal environment, 64 KiB stdout/64-GPU limits and a
two-second timeout with kill/reap. Worker/MCP/tunnel secrets and Python/loader
injection variables are excluded. Shutdown joins owned collection; a four-second
timeout reports pending cleanup. A dropped final owner closes sampling after its
current bounded diagnostic. Arbitrarily stalled OS filesystem calls are not
claimed to be forcibly cancellable.

An actual Windows test initially found NVML initialization failed after environment
clearing. Controlled single-variable probes identified ProgramFiles as necessary;
LC_ALL plus SystemRoot/ProgramFiles on Windows now suffice. The environment
exclusion test proves known product-secret/Python-injection sentinels are absent
in the diagnostic child. No full inherited environment workaround was used.

## Final checks

| Check | Evidence/result |
|---|---|
| Windows check.ps1 including actual WSL check.sh | `.gate.txt`, exit 0 |
| Native Rust workspace | 88 unique tests; fmt/check/clippy PASS |
| Linux Rust workspace | 206 unique tests; fmt/check/clippy PASS |
| Python worker | Windows 163 PASS + eight Unix skips; Linux 171 PASS |
| Windows/WSL source identity | 138 files match before/after gate; `.sources.json` |
| CPU/reset/guest/memory/cgroup/GPU units/invalid data | Component tests PASS |
| Slow collector, 1,024 cache reads, one collection, owned shutdown/final-owner drop | Component tests PASS |
| Diagnostic timeout/overflow/failure child reaping and environment exclusion | Component tests PASS |
| Host stopped/silent/malformed/unsupported Linux resource replies | Native Named Pipe tests PASS |
| Runtime resource parameter rejection/cache reads/no Session startup | Linux endpoint component PASS |
| Actual Windows and Linux NVIDIA collectors | `.windows.txt`, `.linux.txt`, both exit 0 |
| Independent Windows CIM/Linux procfs/NVIDIA reference | `.verify.py`, PASS |
| Real Named Pipe Host/WSL aggregation and notebook proof | `.real.txt`, exit 0 |
| Independent standard-ipynb/output/attachment/shutdown-edit/worker-exit check | `.postconditions.txt`, PASS |
| Final Host executable background STOPPED/RUNNING/reconnect/relaunch | `.background.txt`, PASS |

Nested credential subprocess tests are counted once. The whole gate includes
actual uv/IPython execution, independent Notebook concurrency/namespace isolation,
same-primary FIFO and admission/control responsiveness, failure retention,
atomic document persistence, package cancellation/reconciliation and shutdown.

The independent reference proves Windows RAM total **68,519,723,008 bytes** and
Linux-visible total **20,972,179,456 bytes** agree with the corresponding collectors
and the actual Host aggregation. Both views identify the same RTX 5090 UUID and
**32,607 MiB** reported GPU capacity. Dynamic usage is sampled at different times
and is not required to be equal. This development WSL context did not provide
readable cgroup memory counters; its cgroup metric correctly remains READ_FAILED.
Per-process GPU attribution remains explicitly NOT_SUPPORTED.

Actual isolated WSL root:
`/home/ymsaki/.cache/cokernel-v1-dev/host-proof-f21646eec200`.
Native states: `target/dev-tools/host-metrics-state` and
`target/dev-tools/host-metrics-background-state`.

## Phase 5 requirement-by-requirement audit

| Requirement / ledger row | Authoritative proof inspected |
|---|---|
| Host executable, P5-01 | Final native build and actual background executable proof |
| Owned long-lived WSL bridge, P5-02 | Actual Named Pipe/WSL/IPython run, accepted work across caller disconnect, worker exit |
| Handshake/version compatibility, P5-03 | Fake-runtime malformed/oversized/mismatch/timeout tests and real hello |
| Desired RUNNING/STOPPED persistence, P5-04 | Atomic lock/save/reopen/save-failure tests and final background relaunch |
| Bounded recovery, P5-05 | Failure-count/backoff/circuit-breaker/new-generation/no-request-replay tests |
| Current-user IPC, P5-06 | Protected one-user DACL inspection, actual kernel access denial, peer mismatch/token restoration, request limits/correlation tests |
| Common event/admission/reply flow, P5-07 | Full endpoint/document/Project/control regression suite; actual event/snapshot/inspection/concurrent Session proof |
| User startup settings and independent lifetime, P5-08 | Current full-gate registry/argv/strict-IPC tests; final actual background binary; real sign-in remains L6 |
| OS/GPU aggregation, P5-09 | New component proofs, two-sample real collectors, independent references, actual Host result with source/generation/age |
| Windows import bridge, P5-10 | Actual chunked Unicode-path import, invalid/collision rejection, nbformat/readback/worker-exit independent check |
| Host component corpus, P5-11 | Current full native/WSL gate, including accepted work after cancellation, fake Runtime failures, secrets, ACL and reconnect |
| Desktop-independent lifetime, P5-A-01 | Actual pipe clients disconnect/reconnect with same Session/generation, plus final independent Host executable proof |
| Phase 5 gate | All above proofs PASS; no Desktop process needed to maintain execution and bounded bridge recovery |

This gate proves the Host/bridge/domain boundary. Human and AI origin markers in
Rust proofs are not actual Desktop or MCP clients. It does not close managed-distro
security/provisioning, Windows sign-in/restart, installer/update, Desktop rendering,
Codex/Web ChatGPT connectivity, tunneled access or JAX GPU computation. Those
requirements remain in later phases. Snapshot data is ready for the P8 dashboard;
no dashboard rendering is claimed here.

Source code stayed unchanged after the final gate. Evidence and ledger closure
followed it, so the gate's WBS check records the pre-closure 105-DONE count.
The final ledger is checked independently after the local Phase 5 closure.
