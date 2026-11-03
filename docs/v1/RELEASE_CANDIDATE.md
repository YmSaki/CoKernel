# Windows release candidate

## Intended first-install experience

Download `CoKernelSetup.exe`, install for the current Windows user, and let Setup
prepare the runtime. WSL feature enablement alone asks for elevation; when a
reboot is needed Setup resumes after sign-in. Runtime import, Host, credentials
and user data remain under the original user identity. Setup verifies the image,
checks NVIDIA access, waits for Host HEALTHY, enables Host login startup, and
opens Desktop. In Desktop choose **AIに接続する**, enter the tunnel ID/key and
MCP bearer through the native credential dialog, then connect the corresponding
AI connector. Never enter these secrets into a notebook or a chat message.

AI authoring flow: `create_project` → poll `get_operation` → `create_notebook`
→ `insert_code_cell` → `save_notebook` → `ensure_session` → `execute_cell`
→ poll `get_operation`. Each edit/save/execute supplies the returned document
revision; refresh after execution because persisted outputs change the revision.
`open_notebook` resumes an existing file; its primary Session retains variables
until an explicit restart/stop or a process failure. `add_packages` uses Project
uv and does not silently restart live Sessions. There is no Jupyter server.

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
- This candidate only supports fresh installation. Existing managed runtimes are
  refused before overwriting application files. Runtime migration, repair of a
  partial import, and data/credential removal are not implemented by this package.
- Uninstall preserves the WSL distribution, notebooks and credentials. Stop
  sessions and close Host before uninstalling. The preserved runtime prevents
  accidentally treating reinstall as a fresh migration.
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
