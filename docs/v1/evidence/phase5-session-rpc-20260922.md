# Phase 5 concurrent Session RPC evidence — 2026-09-22

Base: `f35ca99711a8d6c102ef9e47b857e0e377f87cbb` plus the working tree.
This closes **P5-07** together with the previous event, package and Project setup
evidence. P5-GATE and actual Desktop/MCP/JAX GPU acceptance remain open.
No commit, PR or issue closure was performed.

## Implementation and contract

Session startup/restart and safe inspection now await worker responses outside
the Runtime bridge admission loop. Responses retain request IDs and may arrive
out of order. Pending/queued responses are bounded to 16; status, interrupt and
stop remain available at that bound. Inspection results are private replies.

Same-notebook ensure requests share one startup. Supervisor startup locks are
per notebook, with a separate shutdown gate, so unrelated notebook startups can
overlap. Runtime lifecycle reservations reject conflicting package mutation,
re-registration, removal and restart. Existing crashed primaries retain their
state/evidence. Stop and bridge termination cancel/join pending tasks and clean
up owned starting processes before completing shutdown. See INTERFACE_CONTRACTS
section 22 and EXECUTION_RUNTIME section 19.

## Verification

Four Linux component tests cover coalescing, independent startup, capacity,
Project/lifecycle exclusion, shutdown, ownership/strict parameters and restart/stop.
The canonical gate now runs `smoke_session_rpc.py` against actual uv/IPython:

1. A held worker startup does not block another notebook's startup. Duplicate
   ensures produce the same primary Session and one actual launcher.
2. Two held ensure requests and 14 inspections fill capacity. Another inspection
   is refused while ping/interrupt finish within two seconds. All correlated
   replies return marker 123456789 after interruption; document persistence
   records INTERRUPTED.
3. Stop cancels a held replacement startup and its process disappears. Another
   Session retains its marker. EOF reaps both a new held launcher and the active
   unrelated worker.

The real Windows `wsl_pipe_smoke` connects three Named Pipe clients to Host/WSL.
It first observes an inspection pending during execution, then requires another
client's snapshot/status response before that inspection finishes. The pending
inspection subsequently returns 123456789. Import, events, accepted execution
after disconnect, output 124, package/setup completion and shutdown/reopen pass.

Isolated Linux root:
`/home/ymsaki/.cache/cokernel-v1-dev/host-proof-fdea7c80f982`.
The independent verifier validates saved nbformat, output, attachment, shutdown
metadata, absence of failed import and worker PID exit.

| Check | Result |
|---|---|
| Windows check.ps1 with actual WSL check.sh | PASS, exit 0 |
| Native Rust workspace | 76 unique tests PASS; fmt/check/clippy PASS |
| WSL Rust workspace | 174 unique tests PASS; fmt/check/clippy PASS |
| Native Python worker | 163 PASS, eight Unix-only skips |
| WSL Python worker | 171 PASS |
| Real worker/Supervisor/notebook/package/Project/Session RPC smokes | PASS |
| Source identity before and after Linux gate | 118 files match |
| Actual Windows Named Pipe/Host/WSL proof | PASS, exit 0 |
| Independent saved document and worker-exit verification | PASS, exit 0 |

The nested credential test subprocess is counted once. The first full-gate attempt
stopped because the development tar copy lacked executable mode on Linux check
scripts; restoring those modes and rerunning the full gate produced the recorded
pass. No product timeout, assertion or lint rule was relaxed. Final evidence and
ledger edits are documentation-only.

Artifacts: [combined gate](phase5-session-rpc-20260922.gate.txt),
[Host and independent verification](phase5-session-rpc-20260922.real.txt),
[source hashes](phase5-session-rpc-20260922.sources.json).

These are Rust clients in a development Ubuntu distro. They do not establish
actual Desktop/Codex/web ChatGPT use, managed distro security, sign-in autostart,
resource metrics or JAX GPU computation. P5-08 and P5-09 are next.
