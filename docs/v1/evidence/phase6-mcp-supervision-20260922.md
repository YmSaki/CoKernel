# Phase 6 Runtime-owned MCP recovery and local gate — 2026-09-22

Base: `f35ca99711a8d6c102ef9e47b857e0e377f87cbb` plus working-tree changes.
The local Phase 6 gate is PASS. P6-10 and the combined P6-09/P6-A-01/P6-GATE
are DONE locally. P3-SUP-15 and P5-12 record lower-layer repairs discovered by
actual integration. No GitHub issue, PR or commit was created or closed.
The overall user goal remains active: actual Desktop/Codex/Web ChatGPT,
managed workload identity/file isolation, secure tunnel, installation/update
and JAX GPU acceptance are not complete.

## Resulting behavior

An optional independent Runtime task owns the MCP process. It launches only the
trusted configured executable, clears inherited environment, establishes a private
process group, validates bounded readiness and probes localhost authentication.
It observes crashes and hangs, kills/reaps the old group, and retries with bounded
backoff. Four consecutive failures open the circuit. Explicit `mcp.restart`
resets the allowance; the last eight sanitized failures remain observable.
Sixty healthy seconds reset the allowance on the next failure. Recovery reuses
the first bound port and persistent bearer. It never recreates Notebook Sessions
or replays accepted operations. Configuration/API failure degrades optional MCP
while local domain/bridge execution remains available.

Host exposes status and MCP-only restart. Read-only MCP status is included in
the existing `get_runtime_status` tool, and the MCP Unix API cannot restart its
own supervisor. Snapshots/events expose state, generation, PID, address and bounded
failure history without subprocess output or credentials. Explicit Runtime stop
joins MCP cleanup before success. See INTERFACE_CONTRACTS section 31 for exact
bounds and startup configuration.

## Actual process proof

`scripts/v1/smoke_mcp_supervision.py` starts Runtime, which itself starts every
MCP process. The harness never creates a replacement MCP. Actual uv/IPython
execution establishes the shared primary Session and value 123456789. SIGKILL
during an accepted running cell leaves that cell successful, worker PID unchanged,
and value 123456790 readable through recovered authenticated HTTP. The endpoint,
bearer and primary identity remain stable. Three further rapid failures reach
FAILED; local execution still advances the value to 123456791. Human-side MCP-only
restart restores HTTP. SIGSTOP then demonstrates health-based kill/reap/recovery.
Invalid port, conflicting socket file and missing executable are also tested
without losing local control. The unrelated socket file is preserved.

Eight Rust supervision tests cover valid/false/oversized readiness, HTTP 200
instead of required 401, silent-start deadline, environment clearing, process
group, crash signal evidence, retry exhaustion, explicit recovery, hung health,
drop/shutdown cleanup, unsafe configuration and bounded history. Existing
official MCP SDK and all fifteen real HTTP tool tests remain green.

## Lower-layer failures found and repaired

The worker inherited arbitrary Runtime environment variables. The actual secret
sentinel test failed before repair; the recorded
[observation](phase6-mcp-supervision-20260922.environment-before.txt) is a tool-output
transcription, not a recreated log. Worker launch now clears inherited environment,
supplies Project-root HOME and Project-local IPython profile, and forwards only
explicit launch/cache essentials. The same process test now proves sentinel and
bearer absence from worker/MCP cmdline/environment. This does not establish
same-UID filesystem isolation; managed deployment must use separate identities.

The first actual Windows Host integration also failed because MCP startup emitted
an event before the hello response. Host assumed the first frame was a response.
The [original Host result](phase6-mcp-supervision-20260922.host-before.txt) and a
[deterministic pre-fix regression](phase6-mcp-supervision-20260922.handshake-before.txt)
record the failure. Host now validates at most 32 startup events within the existing
handshake deadline, retains the sequence watermark, and obtains current state from
the initial snapshot. Tests reject floods, wrong protocol and sequence rollback,
including rollback across hello. Two regression tests pass on Windows and Linux.

The repaired actual Named Pipe → Host → WSL proof starts supervised MCP and invokes
MCP-only restart through Host IPC. PID 2307 changes to 4323 at the same localhost
port 36367, while Notebook worker 3455 and value 123456789 survive. The proof also
retains its original chunked Windows import, events, package completion, client
disconnect, revisioned save and shutdown/reopen checks. Independent validation
confirms valid nbformat, output 124, attachment and shutdown edit, absent failed
import, reaped worker and both MCP PIDs, removed socket, and no bearer in Host logs.

## Final evidence

| Check | Result |
|---|---|
| Actual check.ps1 plus actual WSL check.sh after both repairs | [Full log](phase6-mcp-supervision-20260922.gate.txt), exit 0 |
| Rust fmt/check/clippy/workspace tests | Windows 93 and Linux 225 unique passing test names |
| Python worker | Windows 166 PASS + 8 Unix-only skips; Linux 174 PASS |
| Byte-identical Windows/WSL source before and after | 152 files; [SHA-256 manifest](phase6-mcp-supervision-20260922.sources.json) |
| All fifteen actual authenticated HTTP tools / shared FIFO / isolated parallel Notebooks | Canonical lifecycle smoke PASS |
| Runtime-owned recovery / open circuit / explicit recovery / hung service / local survival | Canonical supervision smoke PASS |
| Actual Windows Named Pipe / Host / WSL / MCP-only restart | [Host output](phase6-mcp-supervision-20260922.host.txt), exit 0 |
| Independent saved-document and child cleanup checks | [Postconditions](phase6-mcp-supervision-20260922.postconditions.txt), exit 0 |

Host fixture preparation is replayable through
[prepare.py](phase6-mcp-supervision-20260922.prepare.py), which extends the existing
disposable Host fixture rather than changing a managed installation. Final Linux
root was `/home/ymsaki/.cache/cokernel-v1-dev/host-proof-1bc6ead64ffb`; native state
was `target/dev-tools/host-mcp-supervision-state-2`. Development Ubuntu/ymsaki is
not managed-WSL isolation evidence. Toolchains are Windows Rust 1.91.1, WSL Rust
1.88.0, uv 0.12.11 and worker Python 3.12.14.

## Local gate audit

P6-01..04's service/SDK/auth/private Runtime API evidence and P6-05..08's complete
tool/schema/bounds/ownership/revision/hostile-inspection/FIFO/parallelism evidence
were rerun in the final gate. P6-10 now supplies actual Runtime-owned recovery.
Together these satisfy P6-09's local contract/security suite and P6-A-01's actual
MCP read of the Human-origin bridge-created value from the unchanged primary.
This closes P6-GATE locally. The Human path here is the real Host/bridge acceptance
harness; it is not a finished Desktop or an actual Codex/ChatGPT user journey.
Issue #32 remains open for Phase 7. D07 also retains its actual Codex connection
criterion. No remote tunnel or public listener was enabled.
