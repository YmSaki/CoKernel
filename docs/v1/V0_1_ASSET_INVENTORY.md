# CoKernel v0.1 → v1 Asset Inventory

Status: **design baseline**  
Migration policy: **fresh v1 install; legacy v0.1 runtime is explicitly destroyed, not upgraded in place**

## 1. Classification

Every v0.1 asset is classified as one of:

- **KEEP** — architecture/implementation is sound enough to carry forward substantially unchanged.
- **KEEP + REFACTOR** — proven behavior is retained but interface/location/ownership changes.
- **REPLACE** — capability remains but v0.1 implementation is not the v1 product implementation.
- **DROP** — v0.1-only mechanism is intentionally removed.
- **NEW** — required by v1 but absent from v0.1.

This is a source/design inventory only. It does **not** imply v0.1 runtime state is migrated. The installed v0.1 WSL distro is treated as disposable legacy state at the v1 boundary.

---

# 2. Architectural assets

| Asset / concept | Decision | v1 treatment |
|---|---|---|
| Dedicated `CoKernel` WSL2 distro | KEEP | Remains the primary Linux compute boundary. Freshly provisioned for v1. |
| WSL2 as Windows GPU Linux runtime | KEEP | Formalized as v1 Compute Plane. |
| Windows drive automount disabled | KEEP | v1 security invariant. |
| Windows executable interop disabled | KEEP | v1 security invariant. |
| Docker as second isolation boundary | KEEP | Retained. |
| Jupyter + MCP + Tunnel service topology | KEEP + REFACTOR | Same logical services; product-owned runtime layout and contracts replace dev checkout assumptions. |
| Browser and AI share same Jupyter Server | KEEP | Core product invariant. |
| Same-existing-kernel attach | KEEP + REFACTOR | Resolver retained; exposed through narrower MCP tools and metadata-preserving wrappers. |
| WSL localhost socket-proxy bridge | KEEP + REFACTOR | Proven real-machine fix; runtime manager owns it. |
| Tunnel starts after MCP healthy | KEEP | v1 startup invariant. |
| No OAuth required for private CoKernel MCP auth | KEEP | Bearer injection remains internal to Tunnel Client. |
| `/opt/cokernel` vs `/workspace/.venv` split | KEEP + REFACTOR | Elevated to explicit control-plane/workspace invariant; UX fixed per Issue #21. |
| uv-managed workspace | KEEP | `pyproject.toml` + `uv.lock` remain source of truth. |
| Git checkout as installed runtime | DROP | v1 uses versioned packaged runtime payload. |
| `git pull` as product update | DROP | v1 Update Manager uses product artifacts/releases. |
| `.env` as long-lived external secret store | REPLACE | Windows Secret Store is source of truth for external credentials. |
| systemd/Docker background processes as WSL lifetime guarantee | DROP | Windows Host explicitly owns lifetime. |

---

# 3. Root Windows entrypoints

## `install.cmd`, `install.ps1`, `install-elevated.ps1`

**Decision: KEEP + REFACTOR, then demote to recovery/developer compatibility.**

What is valuable:

- WSL feature/bootstrap knowledge;
- safe distro detection;
- reboot/resume behavior;
- cloud-init/noninteractive user creation;
- idempotent prerequisite checks;
- NVIDIA/Docker provisioning sequence;
- real-machine diagnostics accumulated during v0.1.

What changes:

- `CoKernelSetup.exe` becomes the product entrypoint;
- installer has an explicit legacy-v0.1 destruction flow;
- runtime payload comes from installer artifacts, not repository sync;
- normal users do not run PowerShell scripts manually.

The scripts may remain as implementation helpers during bootstrap and as recovery tools until their logic is absorbed behind Installer/Host contracts.

## `start.cmd`, `stop.cmd`, `status.cmd`

**Decision: REPLACE as product UX; KEEP as recovery wrappers.**

Desktop/Tray calls Host IPC. Host owns desired state and runtime lifecycle.

Recovery wrappers may call the same Host/runtime contract for debugging.

## `runtime.ps1`, `runtime-keeper.ps1`

**Decision: REPLACE.**

Retain the discovered requirement that an ordinary persistent WSL client is needed for reliable WSL lifetime. The final owner is CoKernel Host, not detached ad-hoc PowerShell scripts.

The existing scripts remain valuable as a compatibility/reference implementation until Host lifetime ownership passes real-machine acceptance.

## `update.cmd`, `update.ps1`

**Decision: REPLACE.**

Retain:

- active-runtime health acceptance concept;
- convergent update philosophy;
- post-update smoke testing;
- Windows localhost verification.

Remove:

- `git pull --ff-only` product update;
- Windows checkout → WSL source synchronization;
- assumption that installed runtime is a Git worktree.

## `sync-wsl-repo.ps1`

**Decision: DROP from product.**

This exists because v0.1 was developed/run from a Windows Git checkout copied into WSL. v1 runtime payload installation makes it unnecessary.

It may remain in a `tools/legacy/` location temporarily if useful for development, but no v1 runtime component may depend on it.

## `verify-windows-loopback.ps1`

**Decision: KEEP + REFACTOR.**

Its checks become part of typed Health Supervisor/Diagnostics acceptance. A script form may remain for recovery.

---

# 4. Linux runtime scripts

## `scripts/bootstrap-ubuntu.sh`

**Decision: KEEP + REFACTOR.**

Retain package/repository/NVIDIA Container Toolkit provisioning logic. Move behind privileged bootstrap contract and version it as part of product runtime.

## `scripts/harden-wsl.sh`

**Decision: KEEP.**

Its policy becomes a formal v1 invariant. Improve idempotence/version reporting as needed.

## `scripts/configure-wsl-loopback-proxy.sh`

**Decision: KEEP + REFACTOR.**

This is a proven real-machine networking solution. Runtime Manager owns reconciliation; unit names/config become versioned product state.

## `scripts/init-env.sh`

**Decision: REPLACE / SPLIT.**

Retain local Jupyter/MCP token generation and CRLF-hardening lessons.

Remove the role of `.env` as the canonical store for user-facing external credentials. v1 separates:

- non-secret runtime config;
- internally generated local tokens;
- Windows-stored external credentials.

## `scripts/up.sh`, `scripts/down.sh`

**Decision: KEEP + REFACTOR.**

Their proven ordering becomes Runtime Manager operations. The shell scripts may initially back `cokernel-runtime start/stop`, but Host should consume a stable typed contract, not free-form terminal output.

## `scripts/doctor.sh`, `scripts/smoke-test.sh`

**Decision: KEEP + REFACTOR.**

These are high-value acceptance assets. Convert results into structured diagnostic checks while preserving CLI forms.

Required additions for v1:

- default workspace kernel assertion;
- `sys.executable` workspace assertion;
- Jupyter extension mutation disabled/read-only assertion;
- Japanese language pack presence/configuration test;
- MCP tool annotation/schema tests;
- narrow variable-inspection safety tests;
- v1 runtime version/layout assertions.

## `scripts/container-entrypoint.sh`

**Decision: KEEP + REFACTOR.**

Retain:

- create minimal `pyproject.toml` when absent;
- `uv sync`;
- `cokernel-workspace` kernelspec registration.

Add/fix:

- deterministic Jupyter default kernel = `cokernel-workspace`;
- control-plane vs workspace environment UX;
- configured JupyterLab locale;
- extension manager disabled/read-only;
- explicit startup validation that workspace Python exists.

## root `up.sh`, `down.sh`, `logs.sh`

**Decision: KEEP as recovery/developer aliases.**

They cease to be primary product UX.

---

# 5. Compose and container images

## `compose.yaml`

**Decision: KEEP + REFACTOR.**

Retain:

- service separation (`jupyter`, `mcp`, `tunnel`);
- loopback-only backend ports;
- Jupyter GPU exposure;
- MCP dependency on healthy Jupyter;
- Tunnel private route to `mcp:4040/mcp`;
- private bearer injection;
- Tunnel startup wait concept;
- no public inbound binding.

Change:

- move under versioned runtime payload layout;
- remove dependence on dev checkout paths;
- formalize config schema;
- ensure v1 Host/Runtime Manager is desired-state owner;
- consider image tags tied to product runtime version instead of only `:local`.

## `docker/jupyter.Dockerfile`

**Decision: KEEP + REFACTOR.**

Add:

- supported JupyterLab language packs at build time, including `jupyterlab-language-pack-ja-JP`;
- immutable/read-only extension policy configuration;
- v1 version metadata/labels;
- tests proving `/opt/cokernel` is control-plane and `/workspace/.venv` is notebook compute.

Do **not** add `pip` to `/opt/cokernel` merely to make runtime Jupyter Extension Manager mutation work.

## `docker/mcp.Dockerfile`

**Decision: KEEP + REFACTOR.**

Continue pinned upstream + local extension. Add v1 extension tests/capability surface.

---

# 6. MCP extension

## `mcp-extension/`

**Decision: KEEP + MAJOR REFACTOR.**

### Preserve

- extension rather than upstream fork;
- same-kernel path normalization/resolution;
- fail-closed connect behavior;
- transport security wrapper keeping DNS-rebinding protection enabled;
- explicit private Compose hostname allowlist.

### Fix immediately for v1

Issue #22 findings become v1 requirements:

1. Preserve explicit ToolAnnotations when replacing/wrapping `use_notebook`.
2. Restore precise `Literal["connect", "create"]` schema.
3. Add regression tests that inspect registered tool metadata.
4. Add `connect_notebook` with existing-only/no-silent-kernel-create semantics.
5. Add `create_notebook` with create-only/no-overwrite semantics.
6. Add safe `list_variables` and `get_variable` inspection primitives.
7. Keep arbitrary execution tools honestly marked high-risk/open-world.
8. Do not fork upstream for this work.

### Safe inspection requirements

`get_variable` must:

- accept Python identifiers only;
- reject attributes/indexing/calls/operators/imports/comprehensions;
- avoid `repr()`/`str()` on arbitrary objects;
- avoid custom iteration/attribute access;
- exact-type-check supported built-ins;
- bound depth, count, string length, and total response size;
- return structured unsupported-type results.

Tests include hostile objects and built-in subclasses.

---

# 7. Documentation

## `docs/ARCHITECTURE.md`

**Decision: REPLACE for v1.**

The v0.1 document remains historical reference. v1 canonical architecture moves to `docs/v1/` and includes Windows Host, clean install, component contracts, workspace/control-plane UX, and later Node/Fabric extension points.

## `docs/WINDOWS_INSTALL.md`

**Decision: REPLACE.**

v1 user documentation begins with `CoKernelSetup.exe`, not `install.cmd`.

A developer/recovery section may retain script instructions.

## `docs/SECURITY.md`

**Decision: KEEP + REFACTOR.**

Retain the threat model and isolation principles, then add:

- Windows Host/IPC trust boundary;
- Windows Secret Store;
- legacy destructive reset behavior;
- installer/update signing assumptions;
- runtime management surface;
- MCP narrow-capability policy.

## `docs/DEBUGGING.md`

**Decision: KEEP + REFACTOR.**

Diagnostics UI/bundle becomes the primary path; manual commands remain advanced recovery.

---

# 8. CI and test assets

## `.github/workflows/ci.yml`

**Decision: KEEP + EXPAND.**

Retain current Linux validation, MCP tests, Compose validation, image build, tunnel image pin checks, PowerShell parsing.

Add v1 jobs/tests:

- Windows Desktop/Host build;
- Installer build artifact;
- IPC contract tests;
- config/schema tests;
- Jupyter image default-kernel test;
- Jupyter language pack/extension-manager policy test;
- MCP annotations/schema tests;
- hostile-object safe-inspection tests;
- runtime payload manifest/version tests;
- legacy detection/reset unit tests (without destructive real WSL on hosted CI);
- diagnostics redaction tests.

Real-machine acceptance remains required for WSL/NVIDIA/localhost/Tunnel behavior that hosted CI cannot reproduce.

---

# 9. New v1 components/assets

The following do not exist in v0.1 and are required.

## NEW-01 — CoKernel Host

Long-running Windows user-session control-plane process owning desired state and WSL lifetime.

## NEW-02 — Host IPC protocol

Versioned current-user-only Named Pipe API.

## NEW-03 — CoKernel Desktop

Tray-resident Windows application/dashboard consuming Host IPC only.

## NEW-04 — Product Installer

`CoKernelSetup.exe` with preflight, reboot resume, legacy reset, fresh WSL provisioning, product payload installation.

## NEW-05 — Legacy Reset flow

Explicitly destroys the v0.1 distro and installs fresh v1. No in-place migration.

## NEW-06 — Windows Secret Store

DPAPI/Credential Manager abstraction for Tunnel API key and future external credentials.

## NEW-07 — Stable Linux Runtime Management Contract

A structured `cokernel-runtime` interface returning machine-readable status/health/metrics rather than requiring Windows to scrape arbitrary shell output.

## NEW-08 — Health Supervisor

Layered health graph, bounded recovery, backoff, crash-loop breaker, actionable error codes.

## NEW-09 — Metrics Aggregator

CPU/RAM/WSL RAM/storage/GPU/VRAM/temp/power/kernel/service metrics.

## NEW-10 — Product Update Manager

Versioned artifact update replacing Git pull/sync.

## NEW-11 — Structured diagnostics bundle

Typed checks/logs with double-layer secret redaction.

## NEW-12 — Jupyter UX policy

- default workspace kernel;
- immutable control-plane extension model;
- bundled language packs including Japanese;
- explicit locale setting.

## NEW-13 — MCP narrow capability surface

`connect_notebook`, `create_notebook`, `list_variables`, `get_variable` plus metadata regression tests.

## NEW-14 — Runtime/product version manifest

Defines:

```text
product_version
runtime_schema_version
image versions
component protocol versions
```

Required for convergent v1-to-v1 updates.

---

# 10. PR #20 prototype inventory

PR #20 was started before the v1 conceptual boundaries were finalized. It must **not be merged as the v1 architecture**.

Potentially reusable implementation ideas:

- Windows dashboard resource fields;
- DPAPI secret-storage code;
- diagnostics ZIP/redaction approach;
- WPF tray/application shell;
- installer artifact build pipeline;
- packaged-runtime discovery concept;
- active-kernel maintenance guard.

Must be redesigned before reuse:

- UI directly controlling runtime scripts instead of Host IPC;
- UI process owning auto-start/runtime behavior;
- source-checkout/script path assumptions;
- installer semantics that preserve the old distro instead of implementing explicit v0.1 Legacy Reset;
- product update path tied to old runtime scripts;
- absence of a dedicated Host/control-plane state machine.

The PR branch may remain as a prototype/reference until useful code is selectively ported after the design baseline is accepted.

---

# 11. Destructive clean-install boundary

For the current development transition, the intended sequence is:

```text
export anything worth keeping from v0.1 workspace (optional)
→ stop old runtime
→ unregister/destroy old CoKernel WSL distro
→ remove obsolete v0.1 Windows runtime state
→ install v1 from scratch
→ run full acceptance suite
```

This deliberately removes migration complexity from v1 implementation. From v1 onward, normal v1-to-v1 updates must preserve user state and follow the Update Manager contract.
