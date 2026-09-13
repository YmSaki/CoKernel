# CoKernel v1 Test and Acceptance Plan

Status: **implementation baseline**

## 1. Test strategy

CoKernel v1 requires layered tests because it spans Windows, WSL, uv, Python/IPython, GPU, MCP, and secure remote access.

Test layers:

```text
L0 static/schema tests
L1 unit tests
L2 component contract tests
L3 WSL integration tests
L4 end-to-end local tests
L5 MCP/tunnel end-to-end tests
L6 real NVIDIA Windows workstation acceptance
```

Normal v1 development and release verification do **not** depend on GitHub-hosted CI. The canonical verification entrypoints are repository-local scripts (`scripts/v1/check.sh` and `scripts/v1/check.ps1`) plus explicit WSL/Windows/GPU acceptance runs on machines controlled by the project. If a self-hosted runner is introduced later, it is only an execution convenience for the same commands and does not replace local evidence.

## 2. L0 — Static and schema tests

Required:

- Rust formatting/lint (`cargo fmt`, `clippy`);
- Rust dependency/build checks;
- Python worker lint/type/test configuration;
- TypeScript lint/build;
- protocol fixture/schema validation;
- documentation link/reference checks where practical;
- no committed secrets;
- Windows/Linux line-ending-sensitive script checks where scripts remain.

## 3. L1 — Unit tests

### Domain

- state transitions;
- Project/Notebook/Session ownership checks;
- environment generation/stale marking;
- error-code mapping;
- resource snapshot formatting.

### Notebook document

- parse nbformat v4 fixtures;
- preserve unknown metadata;
- create valid notebooks/cell IDs;
- revision conflicts;
- atomic save helper behavior;
- path containment and symlink escape rejection;
- import collision behavior.

### Worker

- persistent namespace;
- expression result;
- stdout/stderr;
- exception output;
- rich MIME output;
- top-level async where supported;
- IPython magic/shell semantics selected for v1;
- safe variable serialization;
- hostile object suite;
- output limits.

### MCP

- tool schema/annotation snapshots;
- ID/path validation;
- safe-inspection input rejection;
- response limits.

## 4. L2 — Component contract tests

### uv Environment Manager

Temporary Projects test:

- create/sync environment;
- add/remove package;
- lock generation;
- repeated sync idempotence;
- environment generation increments only when appropriate;
- concurrent/failed operation lock behavior;
- cache behavior uses uv rather than CoKernel custom package copies.

### Session Supervisor

Use real and fake workers:

- start/handshake;
- FIFO execution queue;
- two Sessions execute concurrently;
- interrupt;
- graceful stop;
- forced crash;
- heartbeat timeout;
- FailureRecord capture;
- one crash does not affect another Session.

### Host

Against fake Runtime bridge:

- desired state;
- reconnect/backoff;
- Named Pipe ACL/IPC;
- event forwarding;
- secret values not echoed;
- UI disconnect does not stop runtime.

### Runtime API

- protocol handshake;
- version mismatch;
- request IDs;
- cancellation;
- malformed/oversized frame rejection.

## 5. L3 — WSL integration tests

Run on a controlled Windows development/test machine where WSL is available. Hosted CI is not part of this validation path.

Validate:

- dedicated distro bootstrap fixture/path;
- Host can establish persistent WSL bridge;
- bridge keeps runtime available while desired state RUNNING;
- uv Project creation/sync in WSL filesystem;
- Python worker launch inside Project environment;
- Notebook import bytes streamed from Windows to WSL Project;
- WSL restart produces expected Session-loss/runtime recovery semantics;
- Windows drive automount/interop policy assertions;
- secret file/environment isolation assertions.

## 6. L4 — Local end-to-end journeys

### E2E-01 Project + Notebook

1. Create Project.
2. Add a lightweight package.
3. Create notebook.
4. Execute `x = 123`.
5. Execute `x + 1`.
6. Verify output 124 and same Session.
7. Verify valid `.ipynb` persisted.

### E2E-02 Notebook import

1. Select fixture `.ipynb` from Windows path.
2. Import into WSL Project.
3. Verify cells/metadata/output preservation.
4. Execute imported code using target Project environment.

### E2E-03 Parallel Sessions

1. Same Project notebooks A/B.
2. Start Session A/B.
3. Run overlapping timed workloads.
4. Verify actual concurrency.
5. Verify namespaces isolated.

### E2E-04 Cross-Project isolation

1. Projects with differing package versions/dependencies.
2. Start Sessions concurrently.
3. Verify each imports/resolves its Project state.

### E2E-05 Environment change

1. Start Session.
2. Add package.
3. Verify Session stays alive and is marked stale.
4. Restart Session.
5. Verify package available in new environment generation.

### E2E-06 Crash evidence

1. Start Session.
2. Force worker termination from test harness.
3. Verify CRASHED, exit evidence, last operation, resource snapshot.
4. Verify unrelated Session remains alive.

### E2E-07 Human/AI queue simulation

1. Start shared Session.
2. Submit Human operation A and simulated MCP operation B concurrently.
3. Verify accepted order/serialization.
4. Verify B observes state after A.

## 7. L5 — MCP/tunnel tests

### MCP local

- bearer required;
- list Projects/notebooks/Sessions;
- read notebook without starting Session;
- ensure primary Session;
- execute existing cell;
- `get_variable` primitive value;
- hostile/custom object safe rejection;
- revision conflict;
- restart/stop semantics;
- huge output response bound.

### Tunnel

- start MCP first, tunnel second;
- readiness reported;
- remote call reaches MCP;
- tunnel process restart reconnects;
- MCP restart does not kill Python Session;
- tunnel disconnect leaves local Session healthy;
- no tunnel credential in worker/log/diagnostic output.

## 8. L6 — Real NVIDIA workstation acceptance

Release candidate must be exercised on a real supported Windows + NVIDIA machine.

Mandatory:

1. install from `CoKernelSetup.exe`;
2. WSL environment created automatically;
3. no routine manual WSL/Linux setup;
4. uv Project creation succeeds;
5. representative CPU package works;
6. PyTorch GPU smoke test when supported by release compatibility matrix;
7. JAX GPU smoke test when supported by release compatibility matrix;
8. `.ipynb` create/import/edit/save works;
9. persistent Session state works;
10. two Sessions run in parallel;
11. GPU/VRAM dashboard agrees reasonably with NVIDIA tooling;
12. worker crash evidence shown;
13. Human + MCP same-Session proof;
14. secure tunnel remote proof;
15. Windows sign-in/restart recovery behavior;
16. active-Session update guard;
17. diagnostic bundle redaction;
18. stop/restart/repair paths;
19. one explicit v0.1 legacy destructive reset on disposable legacy environment.

## 9. Same-Session canonical proof

Canonical acceptance notebook:

```python
secret_from_master = 123456789
```

Human UI executes the cell.

AI/MCP `get_variable("secret_from_master")` against the notebook primary Session must return:

```text
123456789
```

No second Python process may be created for this proof.

## 10. Parallelism canonical proof

Session A:

```python
import time
started_a = time.time()
time.sleep(5)
finished_a = time.time()
```

Session B starts overlapping work during A.

Acceptance checks timestamp overlap and distinct worker PIDs/session namespaces.

Within a single Session, two submitted operations must instead run in FIFO order.

## 11. Notebook compatibility fixture suite

Fixtures include:

- plain code/Markdown;
- stream outputs;
- exceptions;
- HTML output;
- PNG base64 output;
- SVG;
- attachments;
- unknown notebook metadata;
- unknown cell metadata;
- notebooks with environment/kernelspec-like metadata from external tools;
- large output notebook;
- malformed JSON;
- unsupported major format fixture.

Round-trip tests compare preservation rules rather than byte-identical JSON formatting.

## 12. Safe variable hostile-object corpus

Tests include classes implementing/overriding:

```text
__repr__
__str__
__iter__
__getattr__
__getitem__
__bool__
operators/comparisons
list/dict/str subclasses
```

Safe inspection must not trigger these behaviors merely to return metadata/value.

## 13. Security acceptance

- workload user not root;
- Windows drives not auto-mounted in normal managed distro workload context;
- Windows interop disabled;
- worker cannot read Windows Secret Store;
- worker env/argv contains no tunnel key/MCP bearer;
- Runtime socket permissions restricted;
- import path traversal/symlink escape rejected;
- MCP bearer auth enforced;
- no public inbound listener required;
- diagnostics redaction corpus passes.

## 14. Performance acceptance targets

Before release, establish measured targets rather than guessing them now. At minimum benchmark:

- Desktop launch/Host reconnect;
- Runtime bridge ready time;
- Project `uv sync` warm vs cold;
- Session worker startup;
- execute-cell request overhead excluding user code;
- output streaming latency;
- 2/4/8 concurrent idle Sessions memory cost;
- metrics polling overhead;
- large notebook open/save/import.

Release targets are fixed after first implementation benchmark and then regression-tested.

## 15. Release blockers

v1 release is blocked by any of:

- notebook execution can silently use the wrong Project environment;
- AI can silently get a different Session than Human for the same primary notebook;
- one worker crash kills Runtime/unrelated Sessions;
- package mutation silently kills Session memory;
- notebook import/save can silently overwrite conflict/outside Project;
- tunnel credential reaches worker/diagnostic output;
- remote access failure stops local execution;
- disruptive update can kill Sessions without explicit user decision;
- `.ipynb` written by normal supported operations is structurally invalid.

## 16. Release verification gates

A release build requires recorded PASS results for the following local/self-hosted verification groups:

```text
rust-core
python-worker
frontend-build
protocol-contract
notebook-fixtures
mcp-contract
windows-host-build
installer-build
```

The canonical commands are ordinary repository/toolchain commands, wrapped by `scripts/v1/check.sh` or `scripts/v1/check.ps1` where practical. Release evidence records the machine/OS/tool versions and command results. Real-machine GPU/WSL/tunnel acceptance remains mandatory and is not delegated to hosted infrastructure.
