# CoKernel v1 MCP and Secure Tunnel

Status: **implementation baseline**

## 1. Purpose

MCP exposes CoKernel's existing domain/runtime to AI clients. It does not create a separate notebook execution subsystem.

Remote AI access layers a managed secure outbound tunnel over the local authenticated MCP endpoint.

Core path:

```text
AI Client
  -> Secure Tunnel
     -> CoKernel MCP
        -> Runtime Domain API
           -> Project / Notebook / existing Execution Session
```

## 2. MCP design principles

1. Tools use CoKernel domain concepts.
2. Read-only intents receive read-only/narrow tools.
3. Tool annotations reflect actual side effects; they are never weakened merely to influence a client safety decision.
4. Human and AI execution against one Session uses the same FIFO execution queue.
5. MCP never spawns a hidden alternate worker outside Session Supervisor.
6. MCP responses are bounded.
7. Arbitrary user-code execution remains explicitly high-risk/open-world even when the source comes from a notebook cell.

## 3. Authentication and transport

MCP server uses Streamable HTTP transport on a private/local Linux endpoint.

Requirements:

- local bearer authentication enabled;
- listener not exposed as a public inbound service;
- tunnel routes to the authenticated MCP endpoint;
- tunnel client receives the bearer through controlled product configuration/handoff;
- AI client does not need the private local bearer separately when the tunnel performs the authorized internal routing pattern;
- diagnostics may verify endpoint health without printing bearer values.

## 4. Tool naming model

Recommended v1 names are concise CoKernel domain operations.

### Project/environment

```text
list_projects
get_project
list_packages
get_environment_status
```

Package mutation tools may be exposed in v1 after permission/UI policy is validated:

```text
add_package
remove_package
sync_environment
```

### Notebook document

```text
list_notebooks
read_notebook
get_notebook
```

Mutating document tools, if exposed:

```text
edit_cell
insert_cell
delete_cell
create_notebook
```

All mutations use notebook revision/conflict rules.

### Session

```text
list_sessions
get_session
ensure_session
execute_cell
interrupt_session
restart_session
stop_session
list_variables
get_variable
```

### Runtime/resources

```text
get_runtime_status
get_resources
```

## 5. Minimum v1 MCP surface

The release-critical human/AI shared-state path requires at minimum:

- `list_projects`;
- `list_notebooks`;
- `read_notebook`;
- `list_sessions`;
- `get_session`;
- `ensure_session`;
- `execute_cell`;
- `interrupt_session`;
- `restart_session`;
- `stop_session`;
- `list_variables`;
- `get_variable`;
- `get_runtime_status`;
- `get_resources`.

Document/package mutations can ship in v1 if tests/permission semantics are ready, but the minimum release proof does not require AI to be able to install arbitrary packages.

## 6. Suggested MCP annotations

Exact fields follow the MCP SDK/spec version used during implementation, but semantics are fixed.

| Tool | Read-only | Destructive | Idempotent | Open-world |
|---|---:|---:|---:|---:|
| list_projects | yes | no | yes | no |
| get_project | yes | no | yes | no |
| list_notebooks | yes | no | yes | no |
| read_notebook | yes | no | yes | no |
| list_sessions | yes | no | yes | no |
| get_session | yes | no | yes | no |
| ensure_session | no | no | yes | no |
| list_variables | yes | no | yes | no |
| get_variable | yes | no | yes | no |
| execute_cell | no | yes/side-effecting | no | yes |
| interrupt_session | no | potentially | yes-ish by state | no |
| restart_session | no | yes (memory loss) | no | no |
| stop_session | no | yes (memory loss) | yes by final state | no |
| get_runtime_status | yes | no | yes | no |
| get_resources | yes | no | yes | no |
| add_package | no | yes/changes environment | no | yes (network/package index) |
| remove_package | no | yes | no | no/limited |
| sync_environment | no | yes/changes environment | no | yes (may download) |

The implementation should map these concepts to the exact MCP annotation model and regression-test the registered surface.

## 7. `ensure_session`

Input:

```text
project_id
notebook_id
```

Behavior:

- returns existing live primary Session if present;
- otherwise starts the primary Session through Runtime;
- does not accept arbitrary executable path/PID;
- concurrent calls deduplicate against the notebook primary Session lock.

It is not purely read-only because it may create a process, but it is closed-world and non-destructive with respect to notebook data.

## 8. `execute_cell`

Input:

```text
session_id
notebook_id
cell_id
expected_revision
```

Behavior:

- executes authoritative source currently stored for that cell;
- joins normal Session FIFO queue;
- returns/streams operation ID and output events according to MCP transport capabilities;
- updates NotebookDocument output through Runtime;
- arbitrary side effects of notebook code are possible, therefore capability remains honestly open-world/side-effecting.

The tool intentionally does not take arbitrary `code` under the guise of a cell ID.

## 9. `list_variables`

Returns bounded metadata only, such as:

```text
name
safe type/module classification
supported_for_get_value
```

No arbitrary value `repr` is required.

Private/internal names may be filtered according to a documented rule, but filtering must not claim a security boundary because user code can deliberately create names.

## 10. `get_variable`

Input is one identifier only.

Validation occurs at MCP layer and again in worker.

Allowed value serialization is the exact-type bounded model defined in `EXECUTION_RUNTIME.md`.

Rejected examples:

```text
a.b
a[0]
f()
a + 1
[x for x in y]
__import__('os')
```

Unsupported/custom values return structured metadata without arbitrary user-controlled representation execution.

## 11. Notebook reading bounds

`read_notebook` must support practical notebooks without creating unbounded MCP responses.

Recommended API shape supports:

- summary/cell index;
- selected cells/ranges;
- optional inclusion/exclusion of outputs;
- bounded output payloads;
- explicit truncation metadata.

This prevents a notebook containing huge embedded images/output from producing accidental multi-megabyte tool responses.

## 12. AI/Human execution race semantics

MCP execution does not receive privileged scheduling over Human UI.

Example:

```text
Human Cell A accepted first
AI Cell B accepted second
=> A then B
```

Both clients can observe operation IDs and final states.

Interrupt targets the currently running Session operation, so UI must make cross-client consequences clear.

## 13. Tunnel lifecycle

Remote Access states:

```text
DISABLED
STARTING
CONNECTED
DEGRADED
RECONNECTING
ERROR
STOPPING
```

Startup ordering:

```text
Runtime healthy
 -> MCP local endpoint ready/authenticated
    -> tunnel process starts
       -> tunnel readiness confirmed
          -> CONNECTED
```

Tunnel is never a prerequisite for local Notebook/Session operations.

## 14. Credential flow

Source of truth for user-facing external tunnel credentials: Windows Secret Store.

Conceptual handoff:

```text
Windows Secret Store
 -> Host retrieves only when needed
 -> controlled bridge secret handoff
 -> Tunnel Supervisor/process
```

Prohibited:

- Project files;
- notebook metadata;
- worker environment;
- ordinary command-line arguments;
- diagnostics/log output.

Runtime may hold credential material only as narrowly/temporarily as required by the tunnel process implementation.

## 15. Local MCP bearer

CoKernel generates a local MCP authentication secret as product runtime state.

It is readable only by Runtime/MCP/Tunnel identities that require it.

Notebook workers do not receive it.

Rotation/restart semantics should be product-managed; users normally do not copy/paste the local bearer.

## 16. Tunnel failure behavior

On remote network/control-plane failure:

- MCP local service may remain healthy;
- tunnel transitions to DEGRADED/RECONNECTING;
- local Sessions continue;
- Human UI continues;
- retry uses bounded exponential backoff;
- repeated failures are visible without flooding notifications/logs.

## 17. MCP failure behavior

If MCP crashes:

- Human/local compute continues;
- Runtime Supervisor restarts MCP according to bounded recovery policy;
- tunnel readiness becomes degraded until MCP is ready again;
- active Python Sessions are not killed merely to recover MCP.

## 18. Security test requirements

Mandatory tests:

- all registered tool annotations match intended behavior;
- unknown/invalid IDs cannot cross Project/Notebook/Session ownership boundaries;
- revision conflicts prevent blind AI overwrite;
- `get_variable` hostile-object suite;
- huge notebook/output response bounds;
- local bearer required;
- bearer never present in worker environment;
- tunnel credential never present in worker environment/log bundle;
- tunnel down leaves local execution healthy;
- MCP restart leaves live Session worker alive;
- Human/AI same-Session value proof.

## 19. Local implementation increment and remaining gates

The Phase 6 service uses `cokernel-mcp serve` and the optional private Runtime
Unix API described in INTERFACE_CONTRACTS sections 29-30. It registers all fourteen
minimum tools plus read-only `get_operation` for execution/persistence completion.
The release increment adds `create_project`, `create_notebook`, `open_notebook`,
`insert_code_cell`, `set_cell_source`, `save_notebook` and `add_packages` (22 total).
AI can author a notebook without a Desktop-created document. Edits require an
exact revision, run through Notebook Document Service, and must be saved explicitly
before execution. `execute_cell` still accepts only an existing cell ID, never
arbitrary source. Package creation/mutation is open-world because uv may execute
package build hooks; package mutation is destructive and non-idempotent.
Package changes preserve existing live Sessions and require explicit restart to
use the changed environment.
Schemas/validation share one contract and annotations preserve section 6's
side-effect distinctions. Notebook reads are revision-aware cell pages with
bounded source previews and opt-in outputs. Full registered contracts are
snapshotted in `protocol/mcp/tools.json`.

The canonical Linux gate includes official SDK interoperation, protocol-version
negotiation, authentication/Origin/Host/body limits, private-file/socket handling,
framing/correlation rejection, and actual service-process crash/restart while a
real uv/IPython primary Session retains its PID and namespace. The process proof
uses test-driven restart. A separate Runtime-owned supervision proof exercises
section 17 recovery, health failure, retry exhaustion and explicit MCP-only restart
as defined in INTERFACE_CONTRACTS section 31. The actual HTTP smoke also covers the canonical Human-to-MCP
variable value, unchanged primary/worker identity, shared FIFO, parallel isolated
Notebooks, Project ownership, revision conflicts, hostile inspection, large
document/value bounds and interrupt/restart/stop. Actual Desktop/Codex/Web ChatGPT
acceptance and managed workload identity separation remain separate requirements.
No remote endpoint or tunnel is enabled by this increment.

## 20. External provider and Windows credential storage

v1 retains OpenAI Secure MCP Tunnel as the outbound provider. The current official
[guide](https://developers.openai.com/api/docs/guides/secure-mcp-tunnels) confirms
private HTTP MCP forwarding and separate Platform tunnel/workspace permissions.
This is a private developer-mode connection; public plugin distribution is outside
this product path. CoKernel must select and verify a pinned tunnel-client release
before enabling supervision. No public proxy or independent execution runtime is
introduced. The local bearer remains an internal routing credential.

External runtime keys are stored by the Windows Host using the current user's
Credential Manager, generic type, local-machine persistence (same user and PC,
not all machine users). One versioned credential blob contains tunnel ID plus key,
so replacement cannot publish mismatched pairs. The target is product-scoped and
derived from the canonical Host state directory; other credentials are never
enumerated. Moving that state directory requires explicit reconfiguration.

The Host configuration methods in INTERFACE_CONTRACTS section 32 never return a
key or automatically enable Remote Access. Credentials are loaded only by the
private Host integration path when a tunnel needs them. Windows Secret Store is
the durable source; Project files and Runtime configuration are not substitutes.
The Host Remote Access owner serializes configuration/control and hands credentials
to the Runtime through the owned bridge; exact contracts are INTERFACE_CONTRACTS
sections 33–34. Actual client access and managed workload isolation remain Phase 7
acceptance work.

Human setup is available through `cokernel-host remote configure` while the Host
is already running. Its native Windows credential dialog collects Tunnel ID as
the user-name field and the provider runtime key as the password. It never asks
for Windows logon credentials. The native adapter accepts 16–255 graphic ASCII
key characters and rejects the Windows credential UI's 256-character boundary
to avoid accepting a potentially clipped paste. The underlying Host API retains
its 2048-byte limit; a later Desktop input path must support that complete bound.
The dialog itself does not persist, prefill from the vault or connect remotely.
Cancellation leaves both stored credentials and remote intent unchanged.
Saving uses the same authenticated Named Pipe `remote_access.configure` operation
as Desktop. It preserves existing intent, so replacing credentials while enabled
reconciles the existing tunnel. Keys are never CLI arguments or output fields.
See [connection runbook](REMOTE_CONNECTION.md) for the actual-client proof.

## 21. Tunnel reconciliation and state authority

Normal lifecycle decisions derive from facts, not from the previous UI label.
The owner has three independent inputs: desired enable/configuration epoch,
MCP readiness plus endpoint generation, and its currently owned child incarnation.
The desired incarnation is `(configuration epoch, MCP generation, endpoint)` when
enabled and MCP is ready; otherwise there is no desired child.

| Owned child | Desired incarnation | Normal action |
|---|---|---|
| absent | absent | wait; show DISABLED when disabled, DEGRADED while the required MCP is unavailable |
| absent | present | prepare and launch that incarnation |
| present | equal to owned incarnation | observe the same process |
| present | absent or different | reclaim the owned child, then reconcile again |

Disable, configuration replacement, MCP loss/replacement and Runtime shutdown
therefore share one ownership rule. A replacement cannot start before the previous
group is killed and the leader reaped. Asynchronous preparation is checked against
the current incarnation before launch; stale preparation cannot start a child.
Repeated enable with identical credentials and repeated disable are idempotent.
An explicit restart creates a new incarnation. Only the tunnel owner publishes
operational state; public snapshots/events contain no credentials.

Failure recovery is a separate policy: process/configuration/health failures permit
three retries after 250 ms, 1 s and 4 s, then ERROR. A new configuration or explicit
restart resets the allowance; an unchanged enable does not bypass the circuit.
Sixty seconds of continuous connection before a process failure renews its retry
allowance. Failure evidence is bounded to eight static-code/time/exit records.
Cleanup failure retains ownership and STOPPING; it cannot publish a successful
stop. An old configuration's failure never consumes a new configuration's allowance.

The current adapter targets the verified official tunnel-client v0.0.14. It uses
only a private Unix health/admin socket, no TCP admin listener, no inherited user
configuration, and no raw process output in product logs. CONNECTED requires exact
`200 ready` plus a successful provider poll observed in the pinned client's metric
`commands_poll_last_successful_timestamp_seconds`. `/readyz` alone, including its
authentication/timeout fallbacks, cannot establish connection. A poll timestamp
unchanged for 45 seconds is stale, measured with monotonic observation time. After
60 seconds initial grace, unhealthy observation displays DEGRADED. The client owns
short network retries; 120 seconds without a healthy observation consumes one
supervisor attempt. These are observations of connectivity, not a guarantee that
an external user has authorized or successfully called a tool.

Remote Access is disabled initially. Runtime credential handoff is transient and
not persisted to Linux files. Control requires the trusted Host bridge, never MCP.
The Host restores the separately persisted Remote Access intent and loads current
Windows credentials for each changed Runtime/configuration target. Remote intent
never overrides an explicit Runtime stop. Managed Linux identity isolation and
actual external clients remain required acceptance.
