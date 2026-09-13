# CoKernel v0.1 -> v1 Asset Inventory

Status: **implementation baseline**  
Migration policy: **fresh v1 install; legacy runtime is disposable and explicitly reset**

v1 is a substantial runtime/product rewrite. This inventory exists to preserve hard-won implementation knowledge without forcing the new architecture to inherit obsolete service boundaries.

## Classification

- **PORT** — behavior/logic is valuable and should be reimplemented in v1 component ownership.
- **REFERENCE** — keep tests/lessons/source available while implementing replacement; do not make v1 depend on it.
- **REUSE** — code/library can likely be carried with limited change after review.
- **RETIRE** — not part of v1 target architecture.
- **NEW** — absent in v0.1 and required by v1.

No classification implies migration of the installed v0.1 distro/state.

## 1. Architecture/concepts

| v0.1 asset/concept | Decision | v1 treatment |
|---|---|---|
| Dedicated CoKernel WSL distro | PORT | Fresh managed v1 distro remains Linux/GPU boundary. |
| WSL GPU through Windows NVIDIA driver | PORT | Keep and validate during install/acceptance. |
| Disable Windows drive automount | PORT | Keep as v1 security invariant. |
| Disable Windows executable interop | PORT | Keep as v1 security invariant. |
| WSL lifetime needs explicit Windows owner | PORT | Implement through long-lived Host<->Runtime bridge instead of keeper scripts. |
| Browser/JupyterLab as primary UI | RETIRE | Windows CoKernel Desktop becomes primary UI. |
| Jupyter Server as notebook runtime | RETIRE | Replaced by CoKernel Notebook Document Service + Session Runtime. |
| Jupyter kernel protocol / ipykernel runtime | RETIRE | Replaced by supervised Project Python/IPython worker protocol. |
| Jupyter MCP Server as AI execution layer | RETIRE | Replaced by CoKernel-native domain MCP service. |
| same-live-state Human/AI invariant | PORT | Reexpressed as shared primary Execution Session. |
| uv-managed user environment | PORT | Expanded: Project is uv environment boundary. |
| `/opt/cokernel` control Python + `/workspace/.venv` split | REFERENCE | Lesson becomes product tooling vs Project environment separation; implementation no longer depends on Jupyter image layout. |
| Docker/Compose second execution boundary | RETIRE as core | Not required for primary Notebook execution; may be reused only if a later component benefits concretely. |
| WSL localhost socket proxy | RETIRE as core | Desktop/Host uses Named Pipe + WSL stdio bridge; no browser localhost Jupyter path required. |
| tunnel starts after MCP healthy | PORT | Keep in Tunnel Supervisor. |
| no public inbound listener | PORT | Keep. |
| internal MCP bearer/tunnel auth isolation | PORT | Keep and strengthen with worker identity separation. |
| Git checkout as installed runtime | RETIRE | Versioned runtime payload. |
| `git pull` product update | RETIRE | Artifact-based Update Manager. |

## 2. Windows installation/runtime scripts

### `install.cmd`, `install.ps1`, `install-elevated.ps1`

**Decision: REFERENCE + PORT knowledge.**

Preserve lessons:

- WSL feature/prerequisite handling;
- elevation/reboot behavior;
- distro detection;
- cloud-init/noninteractive bootstrap;
- Windows path/line-ending pitfalls;
- NVIDIA/Docker-era diagnostic sequencing where still relevant;
- idempotent repair mindset.

v1 product path becomes native installer/bootstrap logic plus minimal scripts only where Windows/WSL tooling makes scripts simpler.

### `runtime.ps1`, `runtime-keeper.ps1`, `start.cmd`, `stop.cmd`, `status.cmd`

**Decision: REFERENCE, then RETIRE from product.**

The discovered lifetime requirement is retained. `cokernel-host` owns the long-lived WSL bridge and desired state instead of detached PowerShell keepers.

### `update.cmd`, `update.ps1`, `sync-wsl-repo.ps1`

**Decision: RETIRE from product.**

Keep lessons around update acceptance and preserving user state. Git checkout synchronization is obsolete in v1.

### `verify-windows-loopback.ps1`

**Decision: REFERENCE only.**

Useful networking diagnostic history, but v1 does not use the Jupyter localhost bridge as its primary Desktop path.

## 3. Linux scripts

### `scripts/bootstrap-ubuntu.sh`

**Decision: PORT.**

Reuse package/bootstrap knowledge after removing assumptions specific to Docker/Jupyter runtime topology. v1 needs uv/product identities/runtime payload/GPU acceptance instead.

### `scripts/harden-wsl.sh`

**Decision: PORT / possible REUSE.**

Automount/interop policy remains applicable. Review for v1 distro/user layout.

### `scripts/configure-wsl-loopback-proxy.sh`

**Decision: RETIRE from core v1.**

The original problem was Windows browser -> Jupyter Docker listener exposure. v1 primary Desktop communication is Named Pipe -> Host -> persistent WSL stdio bridge.

Retain source/history until new Host bridge passes real-machine acceptance.

### `scripts/init-env.sh`

**Decision: RETIRE as architecture.**

`.env` is not the v1 central product configuration/secret model. Relevant token-generation/redaction lessons move to Secret Store/Runtime config.

### `scripts/up.sh`, `scripts/down.sh`

**Decision: REFERENCE.**

Service-ordering and failure-diagnostic lessons are useful. v1 runtime lifecycle is native Runtime/Host state machine.

### `scripts/doctor.sh`, `scripts/smoke-test.sh`

**Decision: PORT test intent, not necessarily script code.**

Convert hard-won checks into structured v1 diagnostics/acceptance tests.

### `scripts/container-entrypoint.sh`

**Decision: RETIRE from primary execution.**

Useful uv sync/kernel-environment lesson only. v1 directly manages Project uv environments and workers.

## 4. Docker/Compose

### `compose.yaml`

**Decision: RETIRE as core v1 runtime.**

Its service ordering/network/security lessons remain reference material. v1 does not require Compose to execute notebooks or run its core local control path.

### `docker/jupyter.Dockerfile`

**Decision: RETIRE.**

Notebook execution no longer depends on JupyterLab/Jupyter Server image.

### `docker/mcp.Dockerfile`

**Decision: RETIRE.**

Replaced by native CoKernel MCP service.

## 5. MCP extension

### `mcp-extension/`

**Decision: REFERENCE, then RETIRE from v1 product.**

Do not lose lessons:

- AI must share exact human execution state;
- attach must fail closed rather than create hidden compute state;
- narrow tools reduce unnecessary high-risk capability exposure;
- annotations/schema precision matters;
- safe variable inspection must avoid arbitrary representation behavior;
- DNS rebinding/Host validation must not be disabled globally merely for internal service names.

v1 reimplements these principles against CoKernel Project/Session domain instead of Jupyter MCP extension hooks.

## 6. Secure Tunnel integration

**Decision: PORT + likely REUSE pinned external tunnel client.**

Retain:

- outbound-only remote access model;
- tunnel credential model;
- readiness probe;
- startup after MCP ready;
- local MCP bearer injection/authorization;
- no OAuth requirement where not needed by the chosen CoKernel auth path;
- admin UI not exposed as a normal product surface;
- reconnect/diagnostic lessons from connection-refused sequencing.

Change:

- target is CoKernel-native MCP endpoint;
- no Compose network dependency;
- Tunnel Supervisor is a v1 component;
- secrets originate from Windows Secret Store and are hidden from workers.

## 7. Documentation/test assets

### `docs/ARCHITECTURE.md`, `docs/SECURITY.md`, `docs/WINDOWS_INSTALL.md`, `README.md`

**Decision: REFERENCE.**

They describe v0.1 and contain useful operational history, but `docs/v1/` is authoritative for the new branch.

### Existing CI

**Decision: PORT infrastructure patterns.**

Keep GitHub Actions habits, Windows + Linux checks, shell/script validation where relevant. Replace Jupyter/Compose-specific gates with Cargo/Python worker/Tauri/protocol/notebook/MCP/installer gates.

## 8. v0.1 bug/acceptance knowledge that must survive

These are more valuable than the exact source files:

1. WSL background Linux processes alone do not guarantee the desired Windows-visible lifetime.
2. Windows<->WSL networking assumptions must be validated on real machines, not inferred from container health.
3. GPU acceptance must run inside the actual workload execution context.
4. Remote tunnel readiness must validate the actual MCP target, not merely a running tunnel process.
5. Startup dependency ordering matters after daemon/WSL restart, not only first Compose launch.
6. Secret-bearing internal auth should be injected/brokered, not handed to AI unnecessarily.
7. Human/AI sharing a notebook file is insufficient; they must share live execution memory state.
8. Coarse MCP capabilities cause unnecessary policy/safety friction; domain operations should be as narrow as practical.
9. Error output must distinguish transport failure, runtime failure, and workload failure.
10. Real-machine acceptance caught issues that static/hosted tests did not.

## 9. New v1 assets

| New asset | Purpose |
|---|---|
| Rust domain/protocol crates | Shared typed domain and wire contracts |
| Windows `cokernel-host` | WSL lifetime/control/secret/update owner |
| Linux `cokernel-runtime` | Project/notebook/session domain runtime |
| uv Environment Manager | Project dependency/environment lifecycle |
| Notebook Document Service | `.ipynb` create/import/edit/save/revision/output |
| Session Supervisor | persistent workers, queue, crash containment |
| Python/IPython worker | interactive Project execution semantics |
| CoKernel-native MCP service | AI access to domain/runtime |
| Tunnel Supervisor | managed remote access lifecycle |
| Tauri Desktop | Project/package/notebook/session UI |
| Windows installer/bootstrap | fresh managed runtime provisioning |
| structured FailureRecord/diagnostics | explain Session/runtime failures |
| versioned update/repair system | stable product lifecycle |

## 10. Removal discipline

Do not delete v0.1 reference code from the branch merely to make the tree visually clean before replacements are proven.

Recommended sequence:

1. implement new component;
2. pass component + real-machine acceptance for the replaced capability;
3. move old implementation to `legacy/` or delete in a dedicated cleanup PR;
4. ensure no v1 path imports/invokes legacy code;
5. update docs/README accordingly.

The installed legacy runtime is disposable; the repository history remains valuable engineering evidence.