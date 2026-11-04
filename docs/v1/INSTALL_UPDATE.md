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

Provision the already-validated D-027 identity boundary: root-owned 0750 Runtime
payload with control primary group and exactly cap_kill,cap_setgid,cap_setuid=ep;
distinct control/workload accounts and the specified supplementary Project group;
2770 Project roots, 2710 Session socket directory and private 0700 control state.
Pass both workload IDs through trusted Host/Runtime configuration. Workloads
cannot execute the privileged Runtime payload. Updates that replace its inode
must restore and verify owner/mode/capabilities before advertising readiness.
Never run Notebook code as root or substitute control identity on configuration
failure. The isolated Phase 7 fixture validates the primitive; fresh-distro
creation, drive/interop hardening and installer/update acceptance remain required.
D-028 requires the reusable provisioning/validation primitives to establish an
owned dedicated Phase 7 connection-test distro before actual remote credentials
are used. The Phase 9 installer drives those same primitives; it is not a second
implementation or permission to modify an existing unrelated distro.

No Linux NVIDIA kernel/display driver is installed inside WSL when Windows WSL GPU support is the required model.

### Connection-validation image primitive (D-028)

`cokernel-bootstrap image` accepts the SHA-256-pinned Ubuntu Base archive and an
explicit development payload archive. It writes a new image and a separate
completion receipt. The image includes disabled automount/interop policy, locked
control/workload accounts, root-owned payload and permission layout before first
boot. The builder bounds archive size/count, rejects collisions, escaping/cyclic
links and entries below links, and never extracts into the host. The payload
includes known Python/uv/worker/tunnel tools; it never copies a user's whole home.
Hash receipts establish development integrity, not signed release provenance.

The development payload must also supply the public CA bundle from Ubuntu
`ca-certificates 20260816ubuntu0.24.04.1`, SHA-256
`64e7c24b0536b525fb19b145768bd5e6e12ced0df3db837c34553ea789acd143`.
The builder verifies this independent pin and installs a root-owned 0644 copy at
`/etc/ssl/certs/ca-certificates.crt` and `/etc/ssl/cert.pem` before boot (the latter
is bundled Python/OpenSSL's default file). Finalization checks both copies.
Private/custom host certificate stores are not copied. TLS verification remains
enabled; package availability through uv's bundled roots does not prove Go tunnel
HTTPS readiness. Future CA updates require an explicit pinned artifact change.

`import-validation` holds the image open against concurrent Windows replacement,
checks its receipt, refuses existing distro names/destination directories and
imports a unique `CoKernelV1-Validation-<image UUID>` environment. Root finalization
checks the marker, file hashes, owners/modes, policy and effective boundary, then
sets and verifies Runtime capabilities. Host configuration is published only on
success. Immutable prepared/imported/provisioned receipts record completed stages.
Import uncertainty retains evidence; there is no automatic retry/unregister/delete.

This primitive requires no systemd. A root WSL boot command protects distro-local
`/run/WSL` as 0700. The launch adapter checks exact root-owned policy, absent Windows
drive mounts and protected socket-directory permissions before Runtime starts,
then clears the environment and supplies explicit product paths/identities.
Changed protection rejects launch. No VM-wide binfmt, Windows registry or other
distro configuration is modified. See SECURITY_OBSERVABILITY section 3.

These are connection prerequisites, not completion of setup UX, machine preflight,
release authenticity, reboot/resume, repair, legacy reset, JAX or actual clients.

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

The Windows Host registers one `REG_SZ` value, `CoKernelHost`, under the current
user's `Software\Microsoft\Windows\CurrentVersion\Run` key. Its command runs the
installed Host with absolute configuration/state paths and `serve ... --background`.
It does not pass `--start`: persisted STOPPED remains authoritative. The Host is a
Windows subsystem executable; background mode neither attaches a console nor waits
for console interrupt delivery. No Desktop process is needed.

The installer/repair CLI is `cokernel-host autostart status|enable|disable
<absolute-config> <absolute-state-directory>`. Desktop uses the Host IPC settings
in INTERFACE_CONTRACTS section 27. Enabling validates the local WSL configuration,
existing executable and bounded quoted command before writing. Disabling removes
only the fixed product value and is idempotent. Neither operation changes desired
state or controls the current Runtime. Uninstall removes this registration before
removing its executable; update/repair refreshes it if installed paths change.

Commands over 260 UTF-16 units are rejected without truncation. Windows may delay
Run entries or suppress them through Startup Apps/policy; registration alone is
not evidence that a real sign-in launched Host. The status API leaves effective
enablement unknown when registered. See Microsoft's
[Run and RunOnce registry keys](https://learn.microsoft.com/en-us/windows/win32/setupapi/run-and-runonce-registry-keys).
Actual sign-in acceptance and installer integration remain required release checks.

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

## 20. Candidate.8 lifecycle contract

Requirements R-UPD-001..004 and section 10 are realized by the following bounded
maintenance path. These are implementation decisions, not a claim of actual
Windows/GPU acceptance.

- New installations use `%LOCALAPPDATA%/CoKernel` and the dedicated `CoKernel`
  WSL name. Existing candidate.7 `CoKernelV1` data roots and unique validation
  distro names remain in place. Moving the Host state directory would change the
  Credential Manager target, so updates never rename it. Both roots present is
  an ambiguity error; a pre-existing unrelated `CoKernel` distro is not adopted.
- Windows binaries are installed under `Programs/CoKernel/versions/<version>`.
  Older binaries remain available for failure recovery. Routine Linux updates
  replace only `/opt/cokernel`, its manifest, and the bundled public CA files.
  Projects, registry, caches, Python environments, WSL registration and virtual
  disk stay in place. This release supports the same pinned Ubuntu base and
  state/protocol schema; a base/schema transition needs an explicit migration.
- Interactive installation asks the user to save edits and approve loss of live
  computations/variables before stopping Desktop/Host. Unattended destructive
  maintenance is refused. A failed graceful stop may terminate only processes
  whose executable paths are under the selected product installation.
- A current-user exclusive file lock serializes maintenance. An atomic Windows
  journal preserves desired state, remote-access intent, prior application
  metadata and login registration. Login startup is removed while changing files.
  Remote AI is disabled during new-runtime health checks, then prior intent is
  restored. STOPPED remains STOPPED after the temporary health check.
- A root-owned Linux staging directory retains the previous product payload.
  The image hash is checked before extraction; extraction selects only product
  paths. Root finalization re-verifies payload hashes/policy/permissions and
  reestablishes Runtime capabilities. Failed or interrupted pre-commit changes
  restore the old payload. The next installer invocation recovers its journal
  before attempting a new update. Successful health validation commits metadata
  before removing rollback files. No automatic user-data schema migration occurs.
- The uninstaller offers Cancel, preserve all compute data, or erase all compute
  data. Erase requires an additional confirmation. Before WSL termination or
  unregister, installation receipts, configured identity and the current user's
  WSL registry BasePath must agree. Foreign or ambiguous registrations fail closed.
  An already absent registration may be accepted only for explicit removal.
- Full removal deletes the application-scoped Credential Manager entry using the
  original canonical Host state directory, unregisters only the owned distro,
  and removes product state/log/cache storage. WSL itself, NVIDIA drivers,
  WebView2 and other distributions are not uninstalled. Default uninstall keeps
  compute data and credentials; reinstall detects and reuses that environment.

Verification: `packaging/test-lifecycle.ps1` covers root selection, candidate.7
ownership, foreign-distro refusal, interrupted-import identification and atomic
journal replacement. `packaging/test-runtime-maintenance.py` executes real
filesystem switches and injects failures at six boundaries to verify rollback
and notebook preservation. These fixtures do not substitute for actual Windows
installation, reboot, update, uninstall and GPU acceptance.
