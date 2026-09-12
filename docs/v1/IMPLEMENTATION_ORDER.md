# CoKernel v1 Implementation Order

Status: **execution baseline after conceptual design approval**

This order minimizes rework by fixing runtime contracts before building the final Desktop/Installer around them.

## Phase 0 — Design freeze

Deliverables:

- `CONCEPTUAL_DESIGN.md`
- `COMPONENT_MODEL.md`
- `V0_1_ASSET_INVENTORY.md`
- this implementation order

Gate:

- v1 is confirmed as a fresh-install boundary;
- v0.1 runtime is explicitly disposable;
- Issue #21 and Issue #22 are accepted as v1 runtime requirements;
- PR #20 is treated only as prototype/reference code.

## Phase 1 — Runtime contract foundation

Create the stable boundary Windows Host will depend on.

Deliverables:

1. runtime/product version manifest;
2. structured runtime status model;
3. structured health model;
4. stable Linux management command surface (`cokernel-runtime` or equivalent);
5. JSON output for status/health/metrics;
6. structured error codes;
7. shell scripts adapted behind the contract rather than exposed as the primary Windows interface.

Gate:

```text
status --json
health --json
start
stop
logs
```

work from inside the managed WSL runtime and are covered by tests.

## Phase 2 — Jupyter workspace UX correctness (Issue #21)

Deliverables:

- `cokernel-workspace` set as deterministic default Python kernel;
- new notebook `sys.executable` points into `/workspace/.venv`;
- control-plane `/opt/cokernel` kept image-owned;
- Jupyter runtime extension install actions disabled/read-only;
- Japanese JupyterLab language pack baked into image;
- locale setting supports Windows-language default and explicit override;
- docs explain control-plane vs workspace Python;
- smoke/CI tests for the boundary.

Gate:

A fresh notebook starts in the uv workspace environment without user kernel selection, Japanese localization can be enabled without installing packages at runtime, and JupyterLab does not offer a broken `/opt/cokernel` PyPI mutation path.

## Phase 3 — MCP capability correctness (Issue #22)

Deliverables:

- preserve `use_notebook` annotations;
- restore precise `Literal["connect", "create"]` input schema;
- tool-metadata regression tests;
- `connect_notebook`;
- `create_notebook` no-overwrite semantics;
- `list_variables`;
- `get_variable` identifier-only safe inspection;
- hostile-object tests;
- bounded serialization tests;
- preserve same-kernel invariant;
- keep upstream execution tools honestly annotated.

Gate:

AI can connect to an existing browser notebook and inspect a primitive variable without needing arbitrary code execution, while compatibility tools remain available.

## Phase 4 — Windows Host

Deliverables:

- long-running Host process;
- desired-state persistence;
- WSL lifetime ownership;
- startup state machine;
- health supervisor/backoff/circuit breaker;
- runtime contract client;
- structured Host logs;
- current-user Named Pipe IPC;
- secret broker interface.

Gate:

With no Desktop UI running, Host can keep desired RUNNING runtime alive, report typed health, recover bounded service failures, and stop cleanly.

## Phase 5 — Windows Secret Store and Tunnel configuration

Deliverables:

- DPAPI/Credential Manager abstraction;
- tunnel credential storage;
- secure handoff to WSL runtime;
- no API key in command line/logs/diagnostics;
- Tunnel startup after MCP healthy;
- `readyz` authoritative status.

Gate:

ChatGPT tunnel reconnects after Host/runtime restart without plaintext credential persistence in the v1 runtime config.

## Phase 6 — Metrics and diagnostics

Deliverables:

- CPU;
- Windows RAM;
- WSL RAM;
- storage;
- GPU model/utilization;
- VRAM;
- temperature/power where available;
- active kernel count;
- service health;
- redacted diagnostics ZIP;
- loopback/GPU/MCP/Tunnel acceptance checks.

Gate:

Host exposes a complete metrics/status snapshot through IPC and diagnostics contain no known secrets.

## Phase 7 — CoKernel Desktop

Deliverables:

- tray-resident Windows UI;
- dashboard;
- Start/Stop/Restart;
- Open Jupyter;
- active kernel view/count;
- logs;
- diagnostics export;
- repair action;
- tunnel settings;
- locale setting;
- auto-start setting;
- update UX;
- actionable errors.

Architectural constraint:

Desktop communicates with Host IPC; it does not directly own WSL/Docker lifecycle.

Gate:

Closing/restarting Desktop does not destroy or orphan the running Runtime.

## Phase 8 — Fresh v1 installer and Legacy Reset

Deliverables:

- `CoKernelSetup.exe`;
- preflight;
- WSL enable/update and reboot-resume;
- v0.1 detection;
- optional legacy workspace export;
- explicit destructive reset;
- `wsl --unregister CoKernel` only inside the confirmed legacy-reset path;
- fresh v1 distro provisioning;
- versioned runtime payload installation;
- Host auto-start registration;
- uninstall behavior for v1;
- setup logs.

Gate:

On the current development machine, the v0.1 environment can be deliberately destroyed and a clean v1 installed without requiring manual WSL/Docker configuration.

## Phase 9 — Product update/repair

Deliverables:

- signed/versioned update artifact model;
- no `git pull` in installed product;
- active-kernel interruption guard;
- v1-to-v1 state-preserving runtime update;
- repair/convergence flow;
- post-update acceptance;
- rollback metadata for Windows binaries/runtime configuration.

Gate:

A v1 installation can update to the next v1 build without recreating the WSL distro or losing workspace state.

## Phase 10 — Full real-machine acceptance

Mandatory acceptance on NVIDIA Windows workstation:

1. fresh setup from Windows installer;
2. WSL distro created automatically;
3. GPU visible in Jupyter container;
4. `uv` workspace works;
5. JAX/PyTorch representative GPU smoke tests where package compatibility permits;
6. default notebook kernel is `/workspace/.venv`;
7. Japanese Jupyter localization works without runtime extension installation;
8. browser/AI same-kernel proof;
9. narrow MCP variable inspection proof;
10. Tunnel ready and reconnect after restart;
11. Desktop close/reopen while Host keeps runtime alive;
12. Windows sign-in automatic recovery;
13. dashboard metrics validated against OS/NVIDIA tools;
14. active-kernel update warning validated;
15. diagnostics redaction validated;
16. stop/restart/repair paths validated;
17. legacy v0.1 destructive reset tested once on the disposable old environment.

## Post-v1

Only after all v1 gates pass:

- CoKernel Node;
- second Windows PC / RTX 3080 node;
- LAN pairing + mTLS;
- remote metrics/lifecycle;
- remote workspace/Jupyter;
- scheduler/Fabric.
