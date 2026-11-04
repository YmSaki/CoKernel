# Candidate.8 lifecycle verification

Candidate.8 adds the install/update/uninstall lifecycle to the candidate.7 AI
notebook authoring surface. This document separates automated evidence from
installed-system acceptance.

## User operations

- Install: run `CoKernelSetup.exe`. New data roots and the dedicated WSL name use
  `CoKernel`. WSL enablement may require Windows restart and setup resumes at login.
- Update candidate.7: keep the existing installation and run the new installer.
  Save edited notebooks before approving maintenance. Live Session variables and
  computations stop. Saved files, Project `.venv`, lockfiles, caches and connection
  credentials remain in the same distro. Existing `CoKernelV1` paths remain valid.
- Uninstall: Windows Settings -> Apps -> CoKernel -> Uninstall. The default answer
  keeps compute data. Full removal requires a second confirmation and removes
  only the verified owned distro, product state and product credential. WSL,
  NVIDIA drivers and WebView2 remain installed.
- Reinstall after preserving data: run the installer; it discovers retained data
  and reuses the managed environment instead of treating it as a conflicting
  first installation.
- Interrupted update: rerun the installer. Its journal distinguishes rollback
  from committed cleanup. Ownership mismatches fail closed; it does not guess
  which unrelated WSL distribution to alter.

## Automated scope

- PowerShell ownership tests: stable new root, candidate.7 retention, both-roots
  ambiguity, mismatched distro/path rejection, partial import discovery by actual
  WSL registration, idempotent removal after unregister, durable journal recovery,
  runtime/remote intent restoration and committed-cleanup handling.
- Filesystem fault injection at six update boundaries: old payload restoration,
  idempotent recovery and unchanged notebook content.
- Packaged Linux image in a disposable root chroot: real bootstrap inspection,
  manifested worker revision, update, rollback, commit, capability finalization,
  unchanged notebook/pyproject/lock/generated files.
- Image ownership regression: inherited textual root/root archive ownership must
  not override the runtime's numeric control GID. The regression test was observed
  failing without the normalization and passing with it. Extraction uses numeric
  owners explicitly.
- Canonical Linux worker/runtime/MCP/tunnel gates, native Windows tests, PowerShell
  parsing, frontend and release builds, and Inno installer compilation.

The chroot uses fixture mount metadata; it is not an installed WSL environment.
The PowerShell ownership tests model the WSL registry; they do not unregister a
real distribution. Actual Windows install/reboot/update/uninstall, actual GPU
computation and real AI connector acceptance remain separate gates. The installer
is unsigned. Only the same pinned base and compatible data/protocol schemas are
supported; no arbitrary schema migration is implied.

## Artifact binding

Tested code commit: `c0c758a7ad2d1a05f9a173a0fd5f52d2f7b32411`.

Workflow: https://github.com/YmSaki/CoKernel/actions/runs/41845662210

Linux job: `126628660458` — success, including packaged-image lifecycle tests.

Packaged runtime image identity: `5e061bfc-995c-4d10-8bda-b10ec6813f80`.

Runtime image SHA-256:
`a124544bcbe1daf48567420191cf36865d743b77deb541feecc0f5f12f6a1635`.

Windows job `126631930981` and release job `126634939966`: success.
All three jobs completed successfully on the tested code commit.

Draft Release:
https://github.com/YmSaki/CoKernel/releases/tag/untagged-cc732dc6c6a4117cfd6e

Installer: `CoKernelSetup.exe`, 72,861,923 bytes.

Installer SHA-256:
`6137b77bf356754a37efa19091512d0e7beeb7f38139c3cfdd8042cc8aacf0c4`.

The workflow artifact ZIP (artifact `13330579453`) was downloaded independently.
The installer bytes were hashed locally and matched both `SHA256SUMS.txt` and
GitHub's release-asset digest. The draft target commit equals the tested code
commit. A subsequent documentation-only commit records this evidence and does
not change the packaged code.

