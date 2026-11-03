# Phase 7 workload environment boundary — 2026-09-22

Base `f35ca99711a8d6c102ef9e47b857e0e377f87cbb` plus current working-tree changes.
This is a P7-06 component checkpoint; managed Linux identity isolation, diagnostics
and actual external clients remain open. No real provider credential was used.

## Change and reason

The uv package runner inherited Runtime environment variables while the worker
launcher already cleared them. Package build hooks are user-selected executable
code, so the difference exposed control credentials and configuration to an
equivalent workload path. `workload.rs` now owns one environment policy used by
both launchers. It supplies Project cwd/HOME/XDG/IPython locations, fixed locale
and progress settings, trusted PATH and explicitly configured uv tool locations.
Native Windows development additionally gets SystemRoot and Project-local profile
and temporary paths. Other inherited variables are excluded.

`UvInvocation` carries its Project root explicitly, so no command-pattern-specific
branch is required to discover launch context. Both Project creation paths reserve
their directory before launching uv. ProjectManager makes its base path absolute
before deriving workload paths.

## Verification and found regression

The real Linux smoke installs a fresh local PEP 517 package through Runtime RPC.
Its backend and subprocess inspect their environment; a persistent Session imports
that installed module and repeats inspection in the worker and another subprocess.
The probe uses synthetic provider/control/index values, invalid control-home uv
configuration, a Python startup marker, uv environment override, proxy and agent
settings. An unfiltered negative control detects inheritance and executes the
marker. The product paths exclude those inputs, retain Project HOME/config,
complete/persist the cell and keep the value `123456789` in the Session.

The first full gate found a CLI Project-creation regression: unlike asynchronous
Project creation, it left directory creation to uv. An explicit Project cwd then
failed at spawn with Windows error 267. Project creation now reserves the directory
exclusively before any uv work. The existing fake-uv test now requires that
precondition instead of silently creating it. Native actual Project smoke passes.
The smoke also decodes captured errors explicitly as UTF-8 with replacement so a
locale decoding failure cannot hide a native command failure.

- [Initial failing gate](phase7-workload-environment-20260922.gate-before.txt).
- Native workload-policy test and four Project tests pass.
- Actual Linux build-hook/worker smoke passes and is now part of check-rust.sh.
- [Final full Windows/WSL gate](phase7-workload-environment-20260922.gate.txt): exit 0; Windows 103 / Linux 237 unique Rust tests; Python 166 + 8 platform skips / 174 passed.
- Source identity before/after Linux verification: 166 files; [SHA256 manifest](phase7-workload-environment-20260922.sources.json).

## Scope of the evidence

Environment exclusion is not denial of control files or same-UID procfs access.
The separate [contract audit](phase7-isolation-contract-audit-20260922.md) identifies
the required identity, socket, filesystem and process-cleanup boundaries. It also
resolves the implementation-order gap: Phase 7 validates that launch primitive;
Phase 9 automatically provisions the same contract in a fresh managed distro.
P7-06 stays IN_PROGRESS and P7-GATE remains open.
