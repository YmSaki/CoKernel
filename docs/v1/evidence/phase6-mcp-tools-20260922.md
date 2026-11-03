# Phase 6 native MCP tools and shared Session proof — 2026-09-22

Base: `f35ca99711a8d6c102ef9e47b857e0e377f87cbb` plus working-tree changes.
P6-05..08 are locally DONE. P3-SUP-04/P3-GATE were reopened after a real
early-interrupt failure, repaired and revalidated. P3-SUP-14 explicitly records
the newly verified Project working-directory boundary. P6-10 (automatic MCP
supervision), P6-09, P6-A-01 and P6-GATE remain open. No commit/PR/issue closure,
public listener, tunnel or user-client configuration was performed.

## Resulting behavior

The authenticated service exposes all fourteen required MCP tools and read-only
`get_operation` to observe asynchronous execution/persistence completion. Calls
use the existing Runtime Unix API and domain authority. One declarative contract
owns schema generation and input validation; the reviewed
[`protocol/mcp/tools.json`](../../../protocol/mcp/tools.json) snapshot covers all
names, descriptions, schemas and side-effect annotations. Execute takes stored
cell identity and revision, never caller-provided source or origin. Restart
explicitly loses volatile memory; inspection accepts one identifier and retains
the worker's exact-type serialization boundary.

Notebook listing returns paginated paths and same-Project open-document identities.
Reading uses the Runtime document owner, preserves revision/state and returns
cell pages with bounded Unicode source previews, opt-in bounded output arrays,
explicit truncation flags and next cell offsets. It does not mutate stored data
or silently open closed notebooks. Success/error results have both JSON-text and
encoded-result bounds; HTTP request IDs are bounded to prevent oversized echo.

## Actual HTTP/worker proof

`scripts/v1/smoke_mcp_lifecycle.py` starts actual Runtime and MCP binaries and a
real uv/IPython worker in a fresh Project. It exercises all fifteen tools:

- the Human-origin Runtime bridge executes `secret_from_master=123456789`;
  MCP reads **123456789** from the same primary Session and worker PID;
- MCP ensure returns that existing primary, with no second worker for it;
- Human A waits on a Project-relative file; MCP B is accepted while A remains
  pending; releasing A yields `human-start`, `human-end`, `mcp` and the expected
  updated variable value, proving shared FIFO without a fixed-duration sleep;
- another Notebook in the same Project executes via MCP while the first is busy,
  using a different worker and isolated variables;
- wrong Project/Notebook/Session combinations, stale revisions, injected source
  and expression variable names fail;
- custom objects and a built-in list subclass return unsupported metadata without
  invoking repr/str/iter/getattr/add hooks; the hook counter remains zero;
- huge Unicode cell source and image output return bounded, explicitly truncated
  pages; a heavily escaped large variable yields CK-MCP-LIMIT;
- interrupt, restart memory loss, stop and re-ensure pass;
- an accepted, executing MCP cell completes after MCP SIGKILL; Runtime, primary
  identity and worker PID remain alive, and MCP can restart with the same bearer;
- bearer content is absent from owned process args/environment/logs; acknowledged
  Runtime shutdown removes the API socket and reaps the worker.

This is an actual native HTTP MCP service/client proof with a Human-origin bridge
test harness. It is not an actual Desktop/Codex/Web ChatGPT user journey.

## Lower-layer failures found and repaired

The initial full gate exposed an IPython-caught runner interruption that returned
`execution_count=null`. The Rust supervisor correctly rejected it and stopped
the worker, so an otherwise ordinary interrupt became a Session failure.
The former worker lifecycle test checked only equality between terminal and
response counts, allowing two nulls to appear valid.

The updated assertion requires a positive integer. A deterministic interruption
inside IPython's runner and empty/whitespace-only cells all failed before the fix
([count regression](phase6-mcp-tools-20260922.count-before.txt)); the actual
Supervisor failure is preserved in
[interrupt gate output](phase6-mcp-tools-20260922.interrupt-before.txt).
The worker now reserves its pre-execution count when IPython omits one and advances
the next counter consistently with its existing outer-interrupt path. It retains
namespace and error status; Rust validation was not weakened. All 37 streaming
tests and the final real Supervisor/worker gate pass.

The deterministic FIFO file barrier also revealed that `uv --project` selected
dependencies while leaving the worker in Runtime's inherited directory. The
[pre-fix gate](phase6-mcp-tools-20260922.cwd-before.txt) timed out at that barrier.
Worker launch now explicitly uses the validated Project root as cwd. The actual
proof checks getcwd against Project identity and successfully resolves the
relative barrier there. This initial cwd is not a filesystem sandbox.

The first snapshot placement also collided with the framed-Runtime fixture
validator ([original output](phase6-mcp-tools-20260922.fixture-before.txt)). MCP
contracts now live in their own protocol/mcp directory and are independently
checked against registered Rust tools. The existing Runtime validator was not
weakened or skipped.

## Final evidence

| Check | Result |
|---|---|
| Actual check.ps1 plus actual WSL check.sh | [full log](phase6-mcp-tools-20260922.gate.txt), exit 0 |
| Native Rust workspace, fmt/check/clippy | 91 unique tests PASS |
| Linux Rust workspace, fmt/check/clippy | 215 unique tests PASS |
| Python worker | Windows 166 PASS + 8 Unix-only skips; Linux 174 PASS |
| Windows/WSL source identity | 149 files equal before/after; [hashes](phase6-mcp-tools-20260922.sources.json) |
| Official MCP Rust SDK | initialization/list/call, exact contracts, auth/Origin/Host/body/ID limits PASS |
| Actual fifteen-tool HTTP proof | canonical smoke in full gate PASS |
| Real Windows Named Pipe/Host/WSL/uv/IPython | [Host output](phase6-mcp-tools-20260922.host.txt), exit 0 |
| Independent saved nbformat/output/attachment/shutdown/worker-exit verification | [postconditions](phase6-mcp-tools-20260922.postconditions.txt), exit 0 |

Native Rust 1.91.1, WSL Rust 1.88.0, uv 0.12.11 and Python 3.12.14 workers were
used. Nested credential subprocess tests are counted once. The final Host proof
used `/home/ymsaki/.cache/cokernel-v1-dev/host-proof-efd7d531777f` and native
`target/dev-tools/host-mcp-tools-state`, without modifying user Projects.

Automatic Runtime-owned MCP recovery remains P6-10; this proof restarts MCP from
the harness. Managed control/workload identities, secure tunnel, actual Desktop
and Codex/Web ChatGPT connections, installer/update and JAX GPU computation remain
required. Same-UID development tests do not establish hostile-worker filesystem
denial of private control credentials.
