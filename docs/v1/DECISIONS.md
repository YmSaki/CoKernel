# CoKernel v1 Architectural Decisions

Status: **implementation baseline**

This file is a compact decision log. Rationale/details live in the referenced specification documents.

## D-001 — Windows application, managed WSL compute plane

CoKernel is presented as a Windows application. Linux compute runs in a dedicated managed WSL distro. WSL is an implementation/runtime boundary, not routine user administration.

## D-002 — Project is the environment boundary

Python dependency/environment state belongs to a Project. Project execution environment is derived from Project dependency state, not selected per notebook by an internal runtime identifier.

## D-003 — uv owns Python dependency/environment management

CoKernel delegates Python/project dependency resolution, lockfile, environment sync, and cache/link behavior to uv. CoKernel does not implement a parallel package resolver/store.

## D-004 — Standard `.ipynb` is the notebook document

Notebook files are standard nbformat-compatible `.ipynb` documents. CoKernel preserves supported/unknown metadata as specified and keeps documents usable by compatible external tooling.

## D-005 — Execution Session is distinct from NotebookDocument

A notebook is durable file state. A Session is volatile process/namespace state. UI/API must never imply that saving a notebook saves arbitrary live Python objects.

## D-006 — One primary live Session per notebook by default

Human and AI deterministically share one primary Session for a notebook. Additional explicit Sessions may be a later feature.

## D-007 — Serialize within Session, parallelize across Sessions

One Session has a single FIFO user-code execution queue. Different Sessions can execute concurrently.

## D-008 — IPython supplies Python notebook execution semantics

Python workers use IPython for persistent interactive execution, expression/rich output/async/magic semantics rather than reimplementing those semantics from raw `exec`.

## D-009 — Worker is a separate supervised process

Notebook code runs in a child Python process. Runtime/control plane survives worker/native-extension failure and captures failure evidence.

## D-010 — Rust is the core/control implementation language

Windows Host, WSL Runtime, Session Supervisor, domain services, protocols, and preferably MCP are implemented in Rust. Python is used where Python execution semantics require it. Desktop uses Tauri/Rust plus a small TypeScript frontend.

## D-011 — Worker structured protocol uses a private Unix socket

Worker control/events do not share stdout with arbitrary user/native writes. Worker stdout/stderr remain capturable diagnostic streams.

## D-012 — Notebook Document Service owns `.ipynb` writes

Workers emit execution events. Runtime applies edits/outputs and writes the notebook atomically with revision/conflict control.

## D-013 — Windows notebook import is a controlled transfer

Import/upload streams bytes from Windows Host to Runtime. Permanent Windows drive mounts are not required for notebook import.

## D-014 — Environment mutations do not auto-kill Sessions

After uv mutation/sync, existing Sessions are marked environment-stale. Restart is explicit because it destroys live memory state.

## D-015 — MCP is CoKernel-domain-native

MCP tools operate on Project/Notebook/Session/Environment/Variable/Resource concepts and call the same Runtime domain API as local clients.

## D-016 — AI does not get hidden compute state

MCP attachment to a notebook reuses its primary live Session. It does not silently create a second Python process when sharing is expected.

## D-017 — Safe variable inspection is identifier-only and bounded

`get_variable` does not accept expressions/eval and does not invoke arbitrary custom object representation behavior.

## D-018 — Secure tunnel is optional to local compute

Tunnel/MCP failure may degrade AI remote access but must not stop local Projects/Sessions.

## D-019 — External tunnel credentials live on Windows

User-facing external credentials are protected by Windows user-scoped secret storage and are not Project/worker state.

## D-020 — Docker is not a core notebook runtime dependency

The v1 execution path is WSL -> uv Project -> supervised Python worker. Containerization may be used only where it provides a specific benefit; it is not required merely to execute notebooks.

## D-021 — Windows Host owns WSL lifetime

A long-lived Host-managed WSL bridge is both the control channel and the explicit Windows-side lifetime owner while desired state is RUNNING.

## D-022 — v0.1 -> v1 is a fresh-install boundary

Legacy experimental runtime may be explicitly destroyed and replaced. Routine v1 updates are state-preserving and artifact-based.

## D-023 — Usability over visual ornamentation

The UI optimizes for clear Project/environment/session behavior, errors, and resource visibility. Visual complexity is not a product goal.

## D-024 — Failure evidence precedes automatic Session replacement

Unexpected worker death is recorded as `CRASHED` with evidence. CoKernel does not immediately hide the failure behind an unexplained restart loop.

## D-025 — Protocols are versioned

Host/Runtime and Runtime/Worker contracts have explicit protocol versions, IDs, structured errors, bounds, and compatibility negotiation.

## D-026 — Main v1 architecture is designed for future Node/Fabric reuse

Runtime domain concepts and local APIs must not depend on Desktop UI or one specific Windows source tree. Future remote Nodes can expose the same Project/Session concepts.

## D-027 — Runtime owns workload credential transition and signal authority

Managed Linux Runtime runs as a non-root control UID with exactly CAP_SETUID,
CAP_SETGID and CAP_KILL effective/permitted, no inheritable capabilities, and a
root-owned 0750 executable whose group is the control primary GID. Workloads use
a distinct non-root UID/GID. Both uv dependency operations and Session launches
apply one pre-exec transition: clear supplementary groups, set all real/effective/
saved workload IDs, clear capabilities, set no_new_privs and arm parent-death
SIGKILL after the credential changes. Existing Runtime child/process-group
ownership remains authoritative for interrupt, stop, cancellation and cleanup.
Signals are syscalls by the Runtime, not an external unprivileged kill program.

This avoids introducing a second privileged launch daemon and its separate
lifecycle/IPC protocol. The tradeoff is explicit: ID-changing capabilities are
powerful, and a compromised Runtime is a privileged control-plane compromise.
The design isolates workload code from control credentials; it is not a claim
that the Runtime itself is an untrusted sandbox. Workloads cannot execute or
modify the privileged Runtime payload, and cannot supply identity settings over
Notebook/MCP requests. Installer/update must preserve root ownership, restricted
execution and the exact capability set. Missing capability/identity/permission
configuration fails before workload execution, never by same-UID fallback.

Kernel basis: [capabilities](https://man7.org/linux/man-pages/man7/capabilities.7.html),
[no_new_privs](https://man7.org/linux/man-pages/man2/PR_SET_NO_NEW_PRIVS.2const.html),
and [parent-death signal](https://man7.org/linux/man-pages/man2/PR_SET_PDEATHSIG.2const.html).

## D-028 — Provision connection-validation prerequisites before real remote keys

Phase 7 real-account acceptance requires a dedicated validation WSL environment
with the product's workload identities, Windows-drive and interop restrictions,
and checked payload/Host launch target. Its minimal reusable provisioning and
validation primitives belong to bootstrap and precede that consumer, even though
the complete installer remains Phase 9. The current-user development distro and
the synthetic different-UID fixture are not substitutes for this environment.

Use an explicitly owned validation distro identity and refuse name/path collisions;
do not overwrite or unregister an existing development, legacy or product distro.
Validate effective workload restrictions using synthetic inputs before handing off
a real key. Phase 9 packages the same primitives into setup, reboot/resume,
update/repair and installation acceptance. This closes a dependency omission in
the schedule without waiving a security gate or changing runtime ownership.
See [prerequisite audit](evidence/phase7-real-connection-prerequisite-audit-20260922.md).
