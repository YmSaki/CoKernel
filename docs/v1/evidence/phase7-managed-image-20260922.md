# E-MANAGED-IMAGE-20260922 — dedicated WSL prerequisite

Date: 2026-09-22. PASS for D-028 bootstrap/image/import and actual Host/worker
boundary in a newly owned validation distro. P7-A-01/P7-GATE remain open.

## Artifact

Rust bootstrap builds without host extraction from Ubuntu Base 24.04.5 amd64 and
explicit Runtime/MCP/bootstrap/uv/tunnel/Python 3.12.14/worker payload. Base SHA-256:
`e77b6f10c2590cef872b33ee9f635a0e3fd1f57fb074c0e52b5c7f56147a0c86`.
Source: [Ubuntu checksums](https://cdimage.ubuntu.com/ubuntu-base/releases/24.04/release/SHA256SUMS).
Development integrity receipts are not signed release provenance.

- Image identity: `4cc2a583-1bfb-43c2-b9c0-74d7a73b2c1d`.
- Image SHA-256: `d8225990b53e188ad898ede051343b3903b8a1af6c956b78618442ecfc130722`.
- Payload SHA-256: `c6be08666410ca64aac8f6b84b51f4832668c0c59010f1784c1f23ad10686a74`.
- Distro: `CoKernelV1-Validation-4cc2a5831bfb43c2b9c074d7a73b2c1d`.
- Config: `target/dev-tools/validation-20260922/installed-v3/host-wsl.json`.
- Installed binary/launch hashes: [postconditions](phase7-managed-image-20260922.postconditions.json).
- Current source: [179 file hashes](phase7-managed-image-20260922.sources.json).

The image predates bootstrap usage-text/comment changes and Windows importer
negative-test/hex validation additions in this checkpoint. Linux provisioning,
launch and Runtime behavior tested here matches that image.

Drive/interop policy, locked accounts and root-owned payload/permissions exist
before first boot. Finalization verifies content/ownership/policy and exact Runtime
capabilities 0xe0. Import refuses collisions and publishes Host configuration only
after finalization. Partial/uncertain artifacts are retained without automatic
unregister, overwrite or re-import.

## Verification

| Evidence | Result and scope |
|---|---|
| [Import](phase7-managed-image-20260922.import-v3.txt) | Exit 0; control UID 61001, workload 61002; GPU device present, not JAX compute |
| [Host](phase7-managed-image-20260922.host-v3.txt) | Actual Named Pipe -> Host -> dedicated WSL -> uv/IPython; Project creation, Windows import, persistent variable/output, async completion, save/flush/reopen, MCP restart preserving worker |
| [Boundary](phase7-managed-image-20260922.boundary-v3c.txt) | Actual notebook: workload IDs, empty groups, zero capabilities, no_new_privs, Project cwd, filtered environment, absent Windows drives; denied control bearer/root shadow/Runtime proc environment/control sockets; Windows PE execution denied with and without explicit WSL_INTEROP |
| [Fail closed](phase7-managed-image-20260922.failclosed-v3.txt) | Exact image/idle processes checked; changing only its interop directory to 0755 rejects launch before protocol output; finally restores guard |
| [Restart](phase7-managed-image-20260922.reboot-v3.txt) | Only owned idle distro terminated; first inspect passes without manual guard repair |
| [After restart](phase7-managed-image-20260922.after-reboot-v3.txt) | Full Host and worker boundary proof passes again |
| [Postconditions](phase7-managed-image-20260922.postconditions.json) | Three successful notebooks retain 124, attachment and shutdown edit; two include boundary cell; failed import absent; no workload process survives stop |
| [Full gate](phase7-managed-image-20260922.gate.txt) | Exit 0; Windows/WSL format/check/Clippy/workspace tests and real execution smokes PASS; bootstrap six Windows/three Linux tests; worker 167+8 platform skips Windows/175 Linux; 179 source files match before/after |

The harmless Windows whoami.exe probe is staged only in this owned distro and
captured output must be empty. No Windows account value was recorded. Synthetic
notebook values and local MCP credentials were used; no external provider key.

## Failures and corrections

First image `ff3158fa-abc7-43e8-a013-fc87cebc1b2b` failed finalization because the
original check interpreted VM-wide WSLInterop registration as distro policy. It
is retained and is not the accepted Host target. Microsoft documents the
[VM registration and socket execution path](https://github.com/microsoft/WSL/blob/master/doc/docs/technical-documentation/interop.md).
The fresh accepted image instead protects distro-local /run/WSL at boot and checks
the boundary before Runtime launch. No VM-wide setting or unrelated distro changed.

The first boundary run collided with the proof's fixed Project name; proof names
are now unique and preserve prior files. The next assumed immediate parent ==
Runtime, but uv is a workload-UID parent. The corrected probe locates the actual
live control-UID Runtime and requires environment denial. Failed evidence remains;
the correction targets the right process without weakening the boundary.

## Remaining work

Actual Codex/Web ChatGPT have not connected. Platform tunnel settings redirected
to login in the app browser; user login is pending. No account/tunnel permissions
or existing tunnel were established. No real key, external tunnel or public
listener was created. Native credential dialog visual acceptance, Desktop, full
installer/update/repair, release authenticity and JAX GPU remain open.

Subsequent real-connection work supersedes the login/key/tunnel status above:
the user logged in, explicitly authorized tunnel creation, and entered credentials
in the native dialog. Actual polling exposed missing OS CA roots in image v3.
Ubuntu's signed ca-certificates package repaired the active environment without
restarting Runtime/Session; reconnect reached CONNECTED with unchanged worker/value.
The image builder now pins and embeds that public CA bundle. Follow-up fresh-image
verification and current client status are recorded in
[real-connection checkpoint](phase7-real-client-20260922.md). The original full
gate/source snapshot above predates this CA correction and the client-preparation
example; their focused checks are recorded separately, not implied by that gate.
