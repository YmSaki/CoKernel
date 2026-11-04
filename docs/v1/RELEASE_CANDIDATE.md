# Windows release candidate

## Intended first-install experience

Download `CoKernelSetup.exe`, install for the current Windows user, and let Setup
prepare the runtime. WSL feature enablement alone asks for elevation; when a
reboot is needed Setup resumes after sign-in. Runtime import, Host, credentials
and user data remain under the original user identity. Setup verifies the image,
checks NVIDIA access, waits for Host HEALTHY, enables Host login startup, and
opens Desktop. In Desktop choose **AIに接続する**, enter the tunnel ID and runtime API key through the native credential dialog, then connect the corresponding
AI connector. Never enter these secrets into a notebook or a chat message.

AI authoring flow: `create_project` → poll `get_operation` → `create_notebook`
→ `insert_code_cell` → `save_notebook` → `ensure_session` → `execute_cell`
→ poll `get_operation`. Each edit/save/execute supplies the returned document
revision; refresh after execution because persisted outputs change the revision.
`open_notebook` resumes an existing file; its primary Session retains variables
until an explicit restart/stop or a process failure. `add_packages` uses Project
uv and does not silently restart live Sessions. If initial setup or dependency preparation fails, use `sync_environment` and
poll its operation to repair the same Project without manual Linux commands.
There is no Jupyter server.

## Build and provenance

`.github/workflows/release.yml` runs only manually, on the dedicated
`release/alpha-installer` branch, or on `v1.*` tags. Linux runs the canonical worker
and runtime gates before assembling a pinned rootfs and release binaries. Windows
runs native tests, builds Desktop/Host/bootstrap, verifies the Microsoft-signed
WebView2 bootstrapper, and packages Inno Setup with SHA256SUMS. The workflow creates
a **draft prerelease**, not a claim of successful real-machine acceptance.

Linux inputs are version/hash pinned in `packaging/build-linux.sh`; the image
builder validates the payload and emits an immutable image receipt. WebView2
Evergreen is signature verified at packaging and installation, with its downloaded
hash included in the payload. Its installation follows Microsoft's documented
[distribution procedure](https://learn.microsoft.com/en-us/microsoft-edge/webview2/concepts/distribution).

## Current boundaries and required acceptance

- Windows 11 x64 with NVIDIA driver and internet access is the initial target.
- Candidate.8 supports fresh installation, candidate.7-to-candidate.8 in-place
  product updates, retained-data reinstall, and preserve-data/full-removal uninstall.
  Read [the lifecycle contract](INSTALL_UPDATE.md#20-candidate8-lifecycle-contract).
- Updates stop active computations only after interactive acceptance. Saved
  notebooks, uv environments/caches and connection settings remain in place.
  Incomplete imports remain fail-closed; explicit full removal can identify a
  registered partial import through its prepared receipt and registered BasePath.
- Default uninstall preserves compute data and credentials. Full removal requires
  a second destructive confirmation and does not uninstall WSL or NVIDIA drivers.
- Installer signing is not configured. SHA256SUMS identifies the artifact, but
  does not substitute for Authenticode publisher identity.
- A build or cross-target check does not prove WSL installation, reboot resume,
  native dialogs, actual tunnel reachability, or NVIDIA computation.

Before publishing the draft, record on the packaged commit: clean Windows install,
WSL-off reboot/resume, existing-WSL preservation, GPU tensor computation via the
actual AI connector, notebook isolation and retained variables, Desktop close/
reopen, login startup, tunnel outage recovery, and no secrets in workload state.
The historical acceptance evidence in WORK_STATUS is not evidence for this build.

## Local development evidence (2026-11-04 JST)

On the Linux development host: MCP contract/authoring unit tests passed; bootstrap
unit tests passed; workspace Clippy passed; Windows cross-target checks passed;
Desktop TypeScript/Vite production build passed; a complete pinned release WSL
image was generated. Socket-based integration is unavailable on this host because
socket creation is denied (EPERM); it is not counted as a passing gate. Native
Windows, GPU and connector acceptance remain separate evidence requirements.

## Candidate build evidence

See [candidate.8 lifecycle verification and artifacts](evidence/candidate-8-lifecycle-20261104.md)
for the exact tested code commit, successful Linux/Windows/release jobs, downloaded
installer verification, and installed-system acceptance that remains open.
[Candidate.7 evidence](evidence/release-candidate-20261104.md) is historical and
is not counted as candidate.8 acceptance.
