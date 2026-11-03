# Phase 7 Linux workload identity primitive — 2026-09-22

Base `f35ca99711a8d6c102ef9e47b857e0e377f87cbb` plus current working-tree changes.
P7-06 component checkpoint; no real provider credential, public endpoint or
installed managed distro was created. Actual external clients remain unproven.

## Implementation

One typed WorkloadIdentity is parsed at Runtime startup and shared by the uv
package runner and Session Supervisor. Managed Runtime is non-root, with exactly
CAP_KILL/SETGID/SETUID and a root-owned control-group 0750 executable. Its existing
supervision remains authoritative. Before workload exec, all supplementary groups,
capabilities and real/effective/saved IDs are reduced to the non-root workload
identity, no_new_privs is set, and parent-death SIGKILL is armed after the ID
transition. Missing/partial/invalid configuration never falls back to control UID.

Project directories and completed notebook files share only the workload group;
control state remains private. Worker socket admission now checks workload UID
as well as kernel PID and existing descendant lineage. A separate control-owned
setgid socket directory permits traversal/connection but not workload listing or
replacement. Runtime's control/MCP socket remains private and control-UID-only.

See D-027, SECURITY_OBSERVABILITY section 4 and INTERFACE_CONTRACTS section 36.
The capability tradeoff is documented: Runtime is a privileged trusted component,
not an untrusted sandbox. No new broker, daemon lifecycle or alternate AI execution
engine was introduced.

## Actual different-UID proof

The explicit root fixture creates a fresh `/tmp/ckuid-*` tree, copies Runtime,
MCP, uv, the verified v0.0.14 tunnel-client and Python 3.12.14, and assigns unused
numeric IDs without changing accounts or system configuration. Only the copied
Runtime receives file capabilities. All fixture processes/files are reclaimed.

Final observed control UID: **8431866**; workload UID: **2931423**. The actual
Runtime retains only its three capabilities. Actual MCP and tunnel processes
have the control UID but zero effective/inheritable/ambient capabilities.

The real PEP 517 backend and its child, persistent worker and its child prove:

- all real/effective/saved UID/GID values equal the workload identity;
- supplementary groups and effective/permitted/inheritable/ambient capabilities
  are empty, no_new_privs is set, and setuid(0) fails;
- opening the control secret and MCP bearer files is denied;
- reading live Runtime, MCP and tunnel `/proc/<pid>/environ` is denied;
- connecting to the private control socket and signalling control processes
  are denied;
- fixture installation/import and cell execution/persistence succeed;
- notebook group access permits workload reads without opening control state;
- IPython profile is created by the workload identity at its configured location;
- interrupt preserves namespace, stop reaps a worker and its child, restart works,
  a crash creates a durable record for the correct Session;
- cancellation reaps uv build descendants, including a child holding output open;
- shutdown reaps Runtime, MCP and tunnel; provider fixture reports no error.

Negative cases reject missing/partial/root/control/sentinel/malformed IDs, missing
CAP_KILL and a Project directory without its group contract before user code runs.

Evidence: [actual proof](phase7-workload-identity-20260922.actual.txt), exit 0.
The tunnel-client connects only to a loopback mock provider with a synthetic key;
this is not Web ChatGPT remote acceptance.

## Failures found and fixed

1. The first worker launch used the development distro's system Python 3.10.12,
   below the worker minimum. The isolated fixture now copies the known 3.12.14
   distribution; no permissions were relaxed to solve it. [Initial failure](phase7-workload-identity-20260922.actual-before.txt),
   [diagnostic evidence](phase7-workload-identity-20260922.python-before.txt).
2. Session interrupt delegated to `/bin/kill`, whose exec correctly dropped
   Runtime's capabilities. It could not signal the other UID. Runtime now issues
   the bounded positive-PID signal syscall directly. The full managed lifecycle
   succeeds. [Failing interrupt checkpoint](phase7-workload-identity-20260922.interrupt-before.txt).
3. IPython silently selected a temporary profile when the configured parent was
   absent. The worker CLI now creates that directory before initializing IPython,
   under workload credentials; a portable test and actual ownership check cover it.

## Regression gate and remaining scope

[Final Windows/WSL canonical gate](phase7-workload-identity-20260922.gate.txt):
exit 0. Windows 104 / Linux 239 unique Rust tests; Python 167 + 8
platform skips / 175 passed. Source identity before/after Linux verification:
169 files; [SHA256 manifest](phase7-workload-identity-20260922.sources.json). The privileged identity fixture is an
additional explicit acceptance command, not a silent skip/claim in the ordinary
unprivileged gate.

P7-06 remains IN_PROGRESS for the complete secret-redaction/diagnostic corpus and
boundary review. Installed managed WSL automount/interop, installer/update handling
of protected payload permissions/capabilities, actual Codex/Web ChatGPT, Desktop
and JAX GPU evidence remain separate required gates. Parent-death signalling does
not claim containment of deliberately escaped process groups.
