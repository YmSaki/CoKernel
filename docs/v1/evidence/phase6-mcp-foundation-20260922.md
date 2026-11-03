# Phase 6 authenticated MCP foundation — 2026-09-22

Base: `f35ca99711a8d6c102ef9e47b857e0e377f87cbb` plus working-tree changes.
P6-01..04 are locally DONE for service startup, HTTP interoperation, bearer
enforcement and the Runtime Unix client. The Phase 6 gate remains open.
No commit, PR, issue closure, public listener or tunnel was created.

## Implemented boundary

The optional Runtime Unix listener submits into the existing RuntimeHandle, with
origin fixed to MCP, bounded connections/frames/deadlines and a narrow allowlist.
Project, Notebook and primary Session ownership stays in the Runtime domain.
The API cannot stop the whole Runtime or mutate environments/documents. Private
directory/socket ownership and kernel peer UID are checked on both ends. A
no-follow private lock plus stale-socket identity checks prevents replacement of
an active listener or unrelated file. Reply loss never causes automatic replay.

The actual `cokernel-mcp serve` binary validates Runtime handshake compatibility
before announcing authenticated loopback readiness. The official rmcp 3.4.0
Streamable HTTP service exposes strict empty-argument `list_projects` and
`get_runtime_status` tools, with truthful read-only annotations. Body/header,
connection and time bounds supplement SDK Host/Origin checks. All HTTP methods
require exactly one Bearer header; no-auth, wrong and duplicate headers fail.

The 256-bit OS-random local bearer is stored as 64 hex characters in a private
regular mode-0600 file under a canonical private directory. Restart preserves it;
unsafe/symlink/invalid credentials fail closed. The service carries only the
credential file path in arguments. Recovery supervision and product-managed
credential provisioning remain later work within the active plan.

## Recorded verification

Canonical command: the existing development Windows `windows-gate.ps1` wrapper
runs actual `scripts/v1/check.ps1`, actual WSL `scripts/v1/check.sh` and compares
Windows/WSL source SHA-256 before and after. Both use native toolchains, not
mocked command output. Full log: [gate output](phase6-mcp-foundation-20260922.gate.txt).
Exit code **0**. Source inventory: [145 hashes](phase6-mcp-foundation-20260922.sources.json).

| Check | Result |
|---|---|
| Windows Rust 1.91.1 workspace fmt/check/clippy/tests | PASS; 88 unique Rust tests |
| WSL Linux Rust 1.88.0 workspace fmt/check/clippy/tests | PASS; 211 unique Rust tests |
| Python worker | Windows 163 PASS + 8 Unix-only skips; Linux 171 PASS |
| Windows/WSL source identity | 145 files equal before/after |
| Official SDK client | initialize, list tools and call both read tools PASS |
| Supported older protocol | 2025-11-25 negotiation returns that version |
| HTTP authentication | unauthenticated GET/POST/DELETE, wrong/duplicate bearer rejected |
| HTTP boundaries | non-loopback bind, invalid Host/Origin and oversized body rejected |
| Bearer persistence | exact length/mode, restart stability, unsafe/symlink/invalid file rejection |
| Runtime socket ownership | live listener/unrelated file preserved; stale owned socket recovered; unsafe directory rejected |
| Unix frames | oversized/zero frame rejected; fragmented client leaves unrelated requests responsive |
| Runtime replies | wrong correlation/protocol/shape and oversized reply rejected |
| Actual Runtime/MCP/uv/IPython | service readiness and same registered Project identity PASS |
| Actual MCP SIGKILL/restart | Runtime survives; primary Session, worker PID and variable value unchanged |
| Credential non-inheritance | bearer absent from owned process args/environment and service logs |
| Explicit Runtime shutdown | worker reaped; private API socket removed |

The five new Linux integration tests reside in
`crates/cokernel-mcp/tests/local_http.rs`. Nested credential subprocess tests in
the wider workspace are counted once. `scripts/v1/smoke_mcp_lifecycle.py` is now
part of the canonical Linux Rust gate and uses actual service binaries and
real uv/IPython. Its temporary Project/credentials/children are owned and cleaned
by the test. A startup handshake parameter omission discovered while reviewing
the first implementation was fixed before this final gate; both the read tool
and actual executable startup are now tested.

The full gate also re-proves same-Notebook primary deduplication/FIFO, different
Notebooks in one Project sharing their package environment while retaining
separate namespaces and overlapping execution, responsive control/admission,
crash evidence, atomic notebook persistence and shutdown cleanup.

## Limits and next work

This increment implements two of the fourteen minimum tools. It does not yet
prove Human execution followed by native MCP `get_variable`; the lifecycle
test reads retained worker values through the Human Runtime bridge. Full MCP
Session tools, ownership/revision/hostile-inspection/bounded-read contracts and
shared Human/MCP execution acceptance remain P6-05..09 and P6-A-01.

The harness restarts MCP after SIGKILL. Runtime-owned bounded recovery required
by MCP_TUNNEL section 17 remains P6-10, now explicitly represented in the ledger.
Development Runtime and worker use one Linux UID, so private file modes and
non-inheritance are not claimed as hostile-worker credential isolation. Managed
control/workload identity separation, Windows secret/tunnel handoff, Desktop,
actual Codex/Web ChatGPT use and JAX GPU computation remain required later gates.
