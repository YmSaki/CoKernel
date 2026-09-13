# CoKernel v1 Install, Update, and Repair

Status: **implementation baseline**

## 1. Product installation goal

The user runs one Windows-facing setup flow and ends with:

- CoKernel Desktop installed;
- CoKernel Host configured for user auto-start according to chosen setting;
- dedicated managed WSL distro provisioned;
- WSL runtime hardened/configured;
- uv and CoKernel runtime payload installed;
- NVIDIA GPU visible to managed Linux runtime;
- first Project can be created without manual Linux setup.

Final distribution artifact name:

```text
CoKernelSetup.exe
```

## 2. Preflight

Installer checks before destructive/provisioning work:

- supported Windows version/build;
- CPU virtualization available/enabled;
- WSL feature/version state;
- pending reboot where relevant;
- free disk space;
- NVIDIA GPU presence for GPU-capable install;
- NVIDIA Windows driver/WSL GPU availability or actionable deficiency;
- existing CoKernel v1 install;
- existing legacy `CoKernel` distro/runtime;
- incompatible distro-name collision;
- network availability for required payloads unless fully bundled/offline mode exists.

Preflight results are user-readable and logged.

## 3. Legacy v0.1 reset

v1 is a fresh product boundary.

If a legacy experimental CoKernel environment is detected, released setup presents explicit flow:

```text
Legacy CoKernel detected
  -> show detected distro/runtime
  -> optionally export wanted notebook/workspace data
  -> warn that WSL unregister permanently destroys distro filesystem
  -> explicit confirmation
  -> terminate legacy processes
  -> wsl --terminate <legacy distro>
  -> wsl --unregister <legacy distro>
  -> remove legacy Windows runtime state/keepers as applicable
  -> provision fresh v1
```

No in-place migration of legacy `.env`, containers, venvs, service units, or Git checkout is promised.

Development acceptance on the current disposable v0.1 installation may intentionally use the destructive path after any wanted files are exported.

## 4. Fresh distro identity

Use a product-controlled distro identity distinct enough to avoid ambiguous manual user distros. Final name should be constant for v1 (e.g. `CoKernel` or `CoKernelV1`) and stored in one shared product constant.

Installer records:

```text
product install version
runtime payload version
distro identity
runtime schema version
install timestamp
```

## 5. WSL configuration

Provisioning establishes:

- systemd where needed by runtime dependencies/support tooling;
- Windows drive automount disabled;
- Windows executable interop disabled;
- workload/runtime users and groups;
- WSL-native Project/cache/state locations;
- GPU path validation;
- product runtime files/permissions;
- logging/state directories.

No Linux NVIDIA kernel/display driver is installed inside WSL when Windows WSL GPU support is the required model.

## 6. Product runtime payload

Installed v1 uses versioned product artifacts, not a mutable Git checkout.

Runtime payload includes:

- `cokernel-runtime` Linux binary;
- `cokernel-mcp` Linux binary or runtime subcomponent;
- Python worker distribution/source bundle;
- configuration templates;
- bootstrap/diagnostic assets;
- version/build metadata;
- tunnel client dependency/pinned distribution according to release packaging.

The exact payload installation path is under `/opt/cokernel` with mutable state elsewhere.

## 7. First-run acceptance

Before reporting success, installer/Host performs:

1. WSL Runtime starts.
2. Runtime protocol handshake succeeds.
3. uv is available.
4. temporary/sample Project environment can be created or a lightweight equivalent self-test passes.
5. Project Python process can run.
6. NVIDIA GPU is visible through supported diagnostic operation.
7. MCP service can start and authenticate locally.
8. if Remote Access configured, tunnel can become ready; if not configured, that is not an install failure.
9. diagnostics/log paths are writable.

Failure reports preserve setup logs and offer retry/repair rather than claiming success.

## 8. Host auto-start

CoKernel Host is a user-level long-running process for v1 Desktop mode.

Auto-start requirement:

- configurable by user;
- starts after Windows sign-in;
- reads desired runtime state;
- if desired state RUNNING, establishes long-lived WSL bridge;
- Desktop UI is independent and may start/show on demand.

Future headless Node mode may require boot-before-login service semantics and is outside single-user desktop v1.

## 9. Desired state persistence

Windows Host persists:

```text
RUNNING | STOPPED
```

A crash/reboot does not reinterpret an explicit user `Stop` as a request to restart automatically.

## 10. Uninstall

Uninstall distinguishes product binaries from user compute data.

Released UX must explicitly state whether uninstall will preserve or delete:

- Projects/notebooks;
- WSL distro;
- uv cache;
- credentials/settings.

Safe default for normal uninstall should preserve user Project data unless the user selects a destructive cleanup option.

A `Remove all CoKernel data` path may unregister/delete the managed distro only with explicit destructive confirmation.

## 11. v1-to-v1 update model

Routine update may include:

- Desktop binary/frontend;
- Windows Host;
- Linux Runtime/MCP binaries;
- Python worker bundle;
- tunnel dependency;
- runtime configuration/schema migrations.

Update artifacts are versioned and integrity-verified.

No normal update runs `git pull` against a development repository.

## 12. Update impact analysis

Before apply, Update Manager determines whether the update requires:

```text
UI restart only
Host restart
Runtime bridge restart
Worker/Session termination
WSL restart
Windows reboot
```

If active Sessions would be lost, UI identifies affected Sessions and requires explicit proceed/defer.

No hidden Session termination for convenience.

## 13. Staged update sequence

Typical disruptive update:

1. download/verify artifact;
2. stage Windows and Linux payloads;
3. ask/defer if active Sessions require termination;
4. stop affected services/Sessions after approval;
5. atomically switch/install Windows components;
6. install/migrate Linux runtime payload;
7. restart Host/bridge/Runtime;
8. run post-update health checks;
9. record successful version;
10. if failure, enter recovery/rollback flow with diagnostics.

## 14. Project data preservation

v1-to-v1 update/repair must treat as user-owned:

- Project source/data;
- `pyproject.toml`;
- `uv.lock`;
- `.ipynb` documents.

`.venv` may be reconstructed if repair requires it and the user is informed when that affects Sessions.

Shared uv cache is performance/storage state and may be rebuilt, but must not be deleted incidentally during routine update.

## 15. Repair

Repair is convergent and component-oriented.

It may:

- verify/reinstall product binaries;
- repair Host auto-start;
- verify WSL distro markers/config;
- restore runtime payload permissions/config;
- verify/reinstall uv/product worker assets;
- reconcile MCP/Tunnel service files;
- test GPU;
- rebuild a Project environment on explicit Project repair.

Repair does not silently delete Project source/notebooks.

## 16. Installer/update logs

Logs include:

- stage and timestamps;
- versions;
- WSL command outcome;
- package/payload verification outcome;
- GPU acceptance;
- migration IDs;
- failure code/evidence.

Logs redact external credentials.

## 17. Reboot/resume

If enabling/updating Windows/WSL requires reboot:

- installer persists a bounded resume marker;
- after reboot/sign-in, setup resumes the correct phase or clearly prompts user;
- repeated resume is idempotent;
- resume state cannot accidentally rerun destructive legacy reset without having recorded the prior explicit authorization/context.

## 18. Update compatibility metadata

Runtime payload contains machine-readable manifest:

```text
product_version
runtime_version
protocol_versions
state_schema_version
worker_protocol_version
supported_python_versions
minimum_windows_build
minimum_wsl_version
```

Host and Runtime handshake uses compatible protocol metadata to fail clearly on partial update mismatch.

## 19. Installer acceptance tests

Mandatory:

- clean supported Windows -> ready runtime;
- reboot/resume path;
- GPU acceptance;
- legacy detection without accidental unregister;
- explicit legacy destructive reset on disposable test machine;
- failed install leaves useful log/retry state;
- uninstall default does not unexpectedly erase Project data;
- active Session blocks/defers disruptive update;
- v1-to-v1 update preserves Project notebooks/locks;
- repair of corrupted runtime payload preserves user Project files;
- partial Host/Runtime version mismatch yields repair/update error, not undefined behavior.