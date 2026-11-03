# Phase 7 workload isolation prerequisite audit — 2026-09-22

Targeted audit of the current working-tree security contract. Authority is
REQUIREMENTS -> ARCHITECTURE / SECURITY_OBSERVABILITY -> INTERFACE_CONTRACTS /
TEST_ACCEPTANCE. IMPLEMENTATION_ORDER and WORK_STATUS schedule those obligations;
they cannot waive them. This audit covers product-credential isolation across
workload launch, file/IPC access and process ownership, not a full security review.

## Findings and required closure

| Finding | Governing contract and current evidence | Minimal counterexample / implication | Closure |
|---|---|---|---|
| Package builds inherited the control environment | SECURITY_OBSERVABILITY §2 places uv environment/workers in workload zone C; §6 excludes product secrets. CommandUvRunner previously added only UV_NO_PROGRESS to an inherited environment. | A local PEP 517 hook reads a provider credential from os.environ without executing any notebook. | Shared workload environment policy now applies to package and worker launches. Real build-hook/child and worker/child proof exists; see the workload environment checkpoint. UID/file boundary remains separate. |
| UID separation was deferred past its consumer gate | ARCHITECTURE §4 recommends distinct control/workload users. SECURITY_OBSERVABILITY §4 deferred exact account layout to installer; Phase 7 requires credential isolation, while Phase 9 provisions identities after Phase 8. | Closing Phase 7 on a same-UID development fixture would approve a guarantee whose enabling boundary does not exist. Strict phase ordering otherwise creates an implicit prerequisite cycle. | Validate the identity/permission launch primitive in Phase 7; Phase 9 packages and provisions the same contract. Phase 7 and release gates remain open. |
| Worker socket permissions assume one identity | session/supervisor.rs creates the containing directory 0700 and socket 0600, then validates peer PID and descendant lineage. | Merely changing the worker UID prevents its connection. Broadly opening the existing control directory would instead expose unrelated state. | A dedicated workload-connectable endpoint must preserve private control state and verify the intended workload UID plus PID/lineage. Do not make the MCP Runtime API workload-accessible. |
| Spawn ownership must include stop and crash cleanup | session/process_group.rs and uv/process.rs signal owned process groups directly. | Launching a worker/build under another UID without a corresponding signal authority can leave accepted stop/cancel unable to reap it. | The chosen launch mechanism must cover start, interrupt, stop, timeout, crash, bridge EOF and descendant cleanup. Validate actual different-UID processes, including a build child holding output open. |
| File ownership is part of the runtime contract | Project service creates directories; Notebook Document Service persists outputs atomically; workloads write Project files and environments. Control sockets/bearer files require private ownership. | A UID-only change can deny uv environment creation or subsequent document persistence; a recursive permissive chmod can expose control credentials. | Define Project shared-access and control-private locations separately, including caches, Python installations, worker payload, atomic document replacement and hostile links. Never repair permissions by recursive ownership changes on user-selected trees. |
| Environment tests do not prove access denial | Current development Runtime, uv and worker use the same Linux UID; local MCP files are owner-only and peer admission is same-UID. | Workload code can attempt to open control files or use same-UID process inspection despite clean environment/argv. | Actual denial tests must target control files, Runtime/MCP sockets and live control/tunnel process environments. No environment-only PASS may close P7-06. |

## Next implementation boundary

Choose and implement one workload launch authority for **both** package operations
and persistent workers. The authority must define credential transition, retained
privileges, descriptor/environment handoff, process identity, signal/reap ownership,
Project access and private IPC. Root workload execution and same-UID fallback are
not acceptable managed-mode recovery paths. A failed or incomplete managed launch
configuration must fail closed before user/package code runs.

Use an isolated validation fixture for the different identities; do not repurpose
the user's existing development distro/account or claim it is the installed
managed distro. Phase 9 remains responsible for fresh managed WSL provisioning,
automount/interop restrictions and repeatable installation. Actual client, Desktop,
JAX GPU and full installed-system acceptance remain required.
