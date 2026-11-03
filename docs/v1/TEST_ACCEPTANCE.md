# CoKernel v1 Test and Acceptance Plan

Status: **implementation baseline**

### Asynchronous package/control regression evidence

The canonical Linux gate runs `scripts/v1/smoke_async_packages.py` using the
actual Runtime, uv and IPython worker. A test wrapper delays only package add:
while it is held, admission returns an operation ID, ping/status/interrupt respond,
the primary Session is preserved and its variables remain readable. Releasing uv
must publish terminal progress without polling, even with the next bridge header
only partially delivered. Explicit package cancellation must reap uv and preserve
Python memory; bridge EOF must cancel/reap uv and save uncertainty before exit.

Rust tests additionally cover per-Project exclusion, the global four-operation
bound, cross-Project get/cancel denial, bounded history, failed/panicked runner
reconciliation, persisted uncertainty on registry reopen, Session startup refusal,
invalid option arguments, symlink escape, uv timeout/output bounds and ordinary
descendant cleanup. Existing Windows Named Pipe proof also observes an async
package completion event and queries/cancels the terminal operation through Host.
`scripts/v1/smoke_async_projects.py` additionally delays actual uv init/sync during
create/register, verifies unrelated Session control, cancellation without file
loss, initialization resumed by sync, stable re-registration identity and recovery
after EOF/Runtime restart. Unit tests also prove a rejected capacity/admission
does not launch uv, and failed registry admission keeps a repairable uncertain
identity.

`scripts/v1/smoke_session_rpc.py` holds a real worker launcher before readiness:
duplicate ensures must share one worker, another notebook must start, and package
mutation/removal/restart conflicts must be rejected. Two pending ensures plus
14 inspections reach the 16-response bound; an extra inspection is refused while
ping and interrupt still complete. Correlated inspection values, interrupted
document persistence, restart/stop cancellation, and EOF cleanup of both starting
and active workers are checked. Linux component tests cover the same bounds,
shutdown, strict parameters and cross-Project ownership. The Windows Host proof
waits until inspection is visibly pending during execution before requiring a
snapshot/status reply from another Named Pipe client, then verifies the value.

The shared Runtime endpoint additionally tests independent clients with duplicate
request IDs, dropped replies/clients without work cancellation, queued and unread
reply capacity, oversized direct requests, owner-stop rejection before admission,
pending launcher cleanup, shutdown refusal with explicit document repair, and
Human/MCP-origin edit conflicts through one revision authority. These are component
clients, not actual MCP-client acceptance. The real Session smoke also runs two
notebooks in one uv Project: they load the same Project package file and base
Python, have distinct PIDs and namespaces, overlap execution, and leave pyproject
and uv.lock unchanged. uv's worker overlay executable paths may differ between
processes; shared Project package origin is verified rather than path-string
equality of those temporary executables.

Document-owner regressions hold its mutex behind a queued filesystem job while
requiring another Session's status, interrupt and stop to complete. They also
queue snapshot, startup validation and execution preparation; reject foreign
notebook ownership without storage access; reject Project removal and conflicting
restart/package mutation; and release storage after stopping to prove cancelled
preparation does not revive execution. A dropped edit reply still saves on owner
shutdown. A cancelled middle preparation cannot let a later MCP-origin execution
overtake an earlier Human-origin preparation, even when the earlier completion
future has not yet been polled. Queue tests prove capacity refusal has no extra
file effect, accepted jobs survive dropped tickets, and an injected job panic
does not prevent subsequent accepted document work. Explicit shutdown with held
storage refuses within its budget and preserves the edit for later recovery.

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

Resource collection proofs include delta-based CPU counters (no double-counted
Linux guest time), memory availability, cgroup ancestor limits, unavailable GPU
fields and MiB conversion, CSV bounds, subprocess timeout/output overflow/reaping,
and credential/injection environment exclusion. A held collector must allow a
snapshot burst without blocking or spawning more collectors, and shutdown must
retain ownership until collection settles. Host resource RPC rejects client paths,
returns Host data with explicit missing Linux data while STOPPED, and remains
usable through silent/malformed/unsupported Runtime metric responses. Real Windows
and WSL proofs require two samples and explicit source/age/scope; the actual Host
pipe proof checks both OS snapshots alongside the Notebook execution/save proof.
GPU workstation evidence compares UUID/capacity with the installed NVIDIA tool;
global device utilization is never evidence of JAX execution or Session attribution.

Autostart component proofs use a UUID-scoped temporary HKCU test key, never the
real Run key: enable/read/disable idempotence, unchanged desired-state bytes,
neighbor value preservation, malformed/oversized/type rejection and exact Windows
argument parsing for Unicode/spaces/metacharacters/trailing separators. Named Pipe
tests reject client-supplied startup paths/commands and refuse unbound Host settings.
`background_host_smoke` additionally launches the actual Host executable with
closed stdio and no console: missing/default STOPPED launches no Runtime, persisted
RUNNING connects real WSL, all clients may disconnect, explicit stop is durable,
and a fresh background launch remains STOPPED. This proof never registers startup
or signs the user out; L6 Windows sign-in/restart and installer acceptance remain
separate requirements.

Against fake Runtime bridge:

- desired state;
- reconnect/backoff;
- Named Pipe protected current-user ACL and actual kernel access denial;
- peer SID verification and thread token restoration before returning to async code;
- malformed/oversized clients and duplicate IDs do not affect other clients;
- per-client request limits, correlated replies and cancelled-read reconnect;
- accepted work survives pipe closure without starting another Runtime;
- actual worker start/output/document-completion events reach another pipe client
  without polling; reconnect snapshots contain the same primary Session;
- event sequence/generation and explicit size/lag recovery; private inspection
  replies are not broadcast, and observation loss does not drive persistence;
- output bursts drain through bounded backpressure without corrupting frames;
- early Session interrupt waits for the worker start acknowledgement; SIGINT at
  start/capture setup and late idle SIGINT preserve valid terminal framing and
  allow the next operation in the same namespace;
- Windows import source bounds/read-handle sharing, failed-transfer cancellation,
  no Windows path forwarding, actual multi-chunk transfer and collision refusal;
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
7. JAX GPU device discovery and completed representative computation through a uv Project Session on the target workstation (R-ENV-008), with dependency/driver versions recorded;
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

Project storage ownership regressions require a held storage job to leave
RuntimeHandle Session status/interrupt responsive, prevent uv launch until
write-ahead persistence, and retain Project exclusion through reconciliation.
Canonical registration aliases must not start concurrent mutations. Failed
registry removal must retain an in-memory retryable identity and all files;
pending removal must reject new document/startup work. EOF must drain admitted
preparation without launching uv afterward. Queue overflow must precede mutation,
and an owner failure must not expose a healthy registry projection.

Session control responsiveness regression tests additionally require:

- a control consumer can remain unpolled while more than the former queue
  capacity of interrupts and stale notifications are admitted synchronously;
- repeated pending interrupts coalesce, and an interrupt after consumption is
  still delivered; stop has priority and an exited actor rejects control;
- a real fake-worker Session running long code accepts an interrupt burst,
  Project stale notification and stop on their first poll, while another
  Notebook in the same Project completes execution and retains its primary;
- worker stop is awaited through graceful/forced cleanup separately from control
  admission; stale state remains observable on the surviving Session.

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

The Phase 6 local foundation additionally requires the official Rust MCP client
to initialize, list tools and call both initial read tools; older supported
protocol negotiation; all-method bearer enforcement; duplicate authentication,
invalid Origin/Host and oversized-body rejection; private/stable credential files;
active/stale/unrelated socket ownership; fragmented/oversized Unix requests; and
incorrect correlation/protocol/result rejection. `smoke_mcp_lifecycle.py` starts
both actual binaries and a real uv/IPython Session, kills only MCP, restarts it,
and verifies unchanged Runtime/worker PID, primary identity and variable value.
It checks bearer absence in owned process arguments/environment/logs, then verifies
explicit Runtime shutdown reaps the worker and removes the socket. This does not
replace full MCP tools, supervised recovery, real Desktop/client acceptance or
the managed control/workload-identity isolation proof.

The MCP tool gate additionally snapshots all fifteen registered schemas and
annotations (fourteen minimum tools plus get_operation), rejects unknown fields,
invalid IDs/revisions/expressions and oversized correlation IDs, and exercises
every tool through actual authenticated HTTP. Human-origin bridge execution must
set secret_from_master=123456789 before MCP reads it from the unchanged primary
Session/worker. Accepted Human A then MCP B must preserve FIFO and visibility.
Another Notebook in the same Project must execute through MCP while the first
is busy, with a distinct worker and namespace. A running accepted MCP execution
must complete after the MCP process is killed. Wrong Project ownership and
stale revisions must fail, hostile/custom/builtin-subclass inspection must not
invoke user hooks, large values must return bounded errors, and huge Unicode
source/image output must return explicit bounded previews without mutating the
document. Interrupt, restart memory loss, stop and subsequent ensure are tested.

Worker lifecycle assertions require a positive integer execution_count, not merely
equality between two null fields. A deterministic IPython runner interruption
before its result counter assignment and empty/whitespace-only cells must each
emit consistent numeric terminal/response counts, reserve the next counter, and
preserve the existing namespace. Real Session Supervisor interrupt/reuse proof
remains mandatory after this worker regression is repaired.

The Runtime-owned MCP gate adds malformed/oversized/unauthenticated readiness,
silent startup deadline, clean child environment, private process group, bounded
retry exhaustion, explicit recovery, stopped-process health detection, and owned
shutdown/drop cleanup. `smoke_mcp_supervision.py` uses actual Runtime/MCP binaries
and uv/IPython: SIGKILL and SIGSTOP must cause automatic recovery at the same
endpoint with stable bearer, primary identity, worker PID and namespace. An
accepted running cell completes after MCP failure; local execution continues
while the retry circuit is open. Invalid MCP config/socket/executable cannot
stop local bridge admission. MCP cannot invoke its own supervisor restart.

Host handshake regression coverage sends startup events before hello, requires
post-hello event sequence continuity, and rejects an event flood, wrong protocol
or sequence rollback. The real Named Pipe/WSL proof enables supervised MCP,
restarts only MCP through Host IPC, and checks unchanged endpoint, worker PID and
shared variable. Its saved Notebook and worker-exit checks remain mandatory.

Windows Secret Store acceptance uses uniquely scoped disposable generic
credentials, verifies actual save/read/reopen/replace/isolation/delete, and cleans
up only those targets. Native Named Pipe tests prove stored key bytes do not echo
in success/error/snapshot responses or Host state files, invalid requests preserve
the old credential, and configuration does not start Runtime. Backend tests cover
bounded versioned data, failed mutation preserving old state, full admission queue,
FIFO and an accepted write surviving loss of its reply. Same-user Windows storage
is not proof of managed Linux worker identity isolation or remote tunnel access.

Tunnel state-model tests enumerate target/owned-incarnation combinations, checking
that normal transitions follow one reconciliation rule and never launch over an
owned child. Real child tests cover MCP-gated launch, identical-intent idempotence,
configuration/MCP replacement, disabled state, bounded retry circuit, explicit
restart, drop cleanup, and ready-without-poll rejection. Unsafe credential files,
executable symlinks, malformed secret input, stale/invalid metrics are rejected.

`python3 scripts/v1/smoke_tunnel_supervision.py <verified-v0.0.14-executable>` runs
the pinned actual client against a loopback-only mock control plane and actual
Runtime/MCP/uv/IPython. It must pass same-primary/PID/value, real provider failure
and reconnect with local execution continuing, MCP replacement, tunnel crash,
disable/shutdown cleanup, denied MCP credential control and secret-free argv,
worker/MCP environments, events and Runtime stderr. This required Phase 7 component
proof is separate from the general local gate because it needs the verified
provider binary. It cannot close Windows Secret Store handoff, managed-identity
isolation, Codex/Web ChatGPT acceptance or P7-GATE.

Host handoff acceptance exercises the real Windows Credential Manager and Named
Pipe with a framed mock Runtime: enable while stopped, bounded nonsecret intent
persistence, identical enable, credential replacement, Runtime crash/restart, Host
restart, explicit reconnect/disable/clear, and sanitized observations. Clear must
retain the credential on unconfirmed cleanup while another client can still use
ordinary Host control. The native WSL proof additionally enables the real pinned
tunnel via Host-stored synthetic credentials, requires CONNECTED through a local
mock provider, preserves the real primary worker/value through remote disable,
and independently checks live process environments/argv and eventual child cleanup.

### Workload package-build environment regression

The Linux canonical gate runs smoke_workload_environment.py with actual uv, a
local PEP 517 backend, its child process and an actual persistent worker/child.
Synthetic control/index/provider credentials, control HOME/XDG configuration,
Python startup injection, uv configuration/environment overrides, proxy and agent
settings are supplied only to the Runtime. An unfiltered negative control must
detect them. The product path must exclude them, use Project HOME/config, install
and import the fixture, preserve Session state and persist the execution. The
probe records only variable names/booleans. Managed UID/file/procfs isolation is
a separate required gate; this test cannot close P7-06 by itself.

### Managed identity primitive proof

`smoke_workload_identity.py <verified-tunnel-client> <Python-3.12-distribution>`
is an explicit privileged Linux fixture setup test, separate from the ordinary
unprivileged canonical gate. It creates only a fresh /tmp fixture, copies binaries
and Python, assigns unused numeric UIDs/GIDs without creating accounts, and adds
the exact capabilities only to its copied Runtime. Runtime runs non-root.

The test rejects missing/partial/root/control/sentinel/malformed identity settings,
missing signal capability and a Project root lacking its group contract. Actual
uv PEP 517 hooks and their children, actual workers and their children must show
the workload IDs, zero capabilities/groups, no_new_privs, denied control-file and
live Runtime/MCP/tunnel environment reads, denied control socket and signal access,
and failed root regain. The actual pinned tunnel uses only a local mock provider.
Installed fixture imports, document persistence and workload read access, workload
IPython profile ownership, interrupt preserving variables, stop/restart/crash
evidence and build-cancellation descendant cleanup must also pass. The fixture
cleans its children and files; it does not modify real accounts or installed
payloads. Full managed WSL/installer, diagnostics and actual-client gates remain.

### Diagnostic projection and credential exclusion corpus

The diagnostic unit corpus places synthetic credentials, bearer/URL forms,
base64/percent/JSON-escaped variants and Unicode text into raw failure fields,
unknown service error codes and timestamps. Export must omit all such text while
preserving useful typed failure facts, unknown/unavailable values and bounds.
The Runtime RPC test holds document storage locked and still obtains diagnostics;
unknown parameters must fail without echoing the supplied secret.

The different-UID real-worker fixture injects a project secret into a crashing
cell ID and proves it is present in local FailureRecord evidence but absent from
diagnostics. It checks all public response/event frames for known synthetic
provider and MCP keys and scans captured Runtime stderr, durable failures and
saved notebooks. Private credential stores are expected to contain their own
credential and are not included in that scan/export. Actual native Named Pipe ->
Host -> WSL diagnostics must identify the existing Session without exposing its
variable value. The full diagnostic ZIP is a later separate acceptance boundary.

### Dedicated managed connection environment (D-028)

Archive tests cover first-boot policy, accounts/permissions, reserved account
collisions, unsafe paths, symlink children and chained link escape/cycles. Native
importer tests reject invalid/oversized/mismatched receipts before WSL invocation
or destination creation, decode UTF-8/UTF-16 output and reject receipt overwrite.

The fresh-image proof imports an owned distro and verifies payload/permissions/
capabilities before publishing Host configuration. Native `wsl_pipe_smoke` then
exercises Project creation, Windows notebook import, execution/persistence, MCP
recovery preserving the worker and stop/reopen through Host. With
`COKERNEL_MANAGED_BOUNDARY_PROOF=1`, its notebook checks workload real/effective/
saved IDs, empty groups, zero capabilities, no_new_privs, Project cwd, filtered
environment, absent Windows mounts, denied control files/Runtime environment/
sockets, and failed Windows PE execution with and without an explicit interop
environment variable. A harmless PE probe is staged only in that owned distro.

`verify_managed_boot.py` requires root, an exact validation image UUID and no live
control/workload processes. It temporarily changes only that distro's interop
directory permissions, requires launch rejection before protocol output and
restores protection in finally. After terminating only this idle owned distro,
inspect and Host/worker proof must pass again without manual guard repair.
These proofs do not close Desktop, actual AI clients, JAX or full installation.

Trust-root acceptance additionally rejects an unpinned CA bundle during image
construction, verifies the installed system copy at finalization, and performs
unauthenticated verified TLS to the real control-plane host from a fresh image.
An actual configured tunnel must reach CONNECTED; readyz alone is insufficient
because local readiness can succeed while authenticated polling has never worked.
