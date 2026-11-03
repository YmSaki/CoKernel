# Phase 7 credential boundary and Runtime diagnostics — 2026-09-22

Base `f35ca99711a8d6c102ef9e47b857e0e377f87cbb` plus working-tree changes.
This checkpoint closes P7-06 and the local disconnect-survival criterion
P7-A-02. Actual external-client acceptance P7-A-01 and P7-GATE remain open.

## Diagnostic contract

`runtime.get_diagnostics {}` returns a bounded typed projection of current
Session/MCP/Tunnel observations. It never loads credentials or raw logs to mask
them afterwards. Names, paths, cell content, variable values, error text and
tracebacks are absent by construction. Unknown textual failure codes/timestamps
become UNKNOWN/null. Session output is limited to 128 entries, service histories
to eight failures each, and the complete response to 128 KiB.

The exclusion corpus embeds synthetic raw, bearer, URL, base64, percent-encoded,
JSON-escaped and Unicode private strings in every free-text failure field.
Classification, failure identity and persistence remain useful without those
strings. A stalled document store does not block the diagnostic operation.
See SECURITY_OBSERVABILITY section 18 and INTERFACE_CONTRACTS section 37.

## Boundary review and evidence

| Boundary | Implementation and verification |
|---|---|
| Windows storage | Host Vault owns Windows Credential Manager access; public configuration is metadata-only. Native API/pipe tests and prior E-SECRET-STORE / E-HOST-HANDOFF prove store, replacement, clear and fixed-error behavior. |
| Host handoff | Secret-bearing frames use private current-user Named Pipe/stdio channels. Host remote owner validates replies and maps errors without echoing arbitrary input; bridge stderr is drained. Host owns intent independently of Runtime restart. |
| Tunnel process | Cleared environment receives only required credentials/configuration; provider key is not an argv value. stdout/stderr are discarded; public failure records contain fixed codes and numeric status. |
| Workload | One shared policy drops UID/GID, groups and capabilities for uv builds and Python workers. Actual build/child and worker/child cannot read control files, live control-process environments or control IPC, or signal control processes. |
| Local failure evidence | Raw local FailureRecords can contain workload text. They exclude inaccessible product keys; they are not themselves shareable diagnostic exports. |
| Shareable diagnostics | Typed projection excludes all workload text and secrets, including unknown service strings. Actual Windows Named Pipe/Host/WSL response preserves Session identity while omitting the known variable name/value. |

[Actual different-UID proof](phase7-diagnostics-20260922.actual.txt), exit 0:
control UID 3533286, workload UID 404622. Real package build and descendants,
worker and descendants, private-file/proc/socket denial, interrupt/stop/restart,
crash persistence, build cancellation and group-owned notebook access pass.
All 147 public response/event frames exclude the synthetic provider key and
the random local MCP bearer. Runtime stderr, FailureRecords and saved notebook
are scanned for these keys. A deliberately private cell ID appears in the raw
failure file but is absent from diagnostics, which retain the matching failure
ID and persisted flag.

[Actual Windows Host proof](phase7-diagnostics-20260922.host.txt), exit 0:
human-style Named Pipe -> Host -> WSL -> worker, diagnostic projection, shared
primary Session and surviving MCP replacement. Worker PID 5124 survives MCP PID
5034 -> 5330. [Independent notebook validation](phase7-diagnostics-20260922.notebook.txt)
confirms nbformat compatibility, saved outputs/attachments/shutdown edit and
worker cleanup. This client is a Rust acceptance client, not Desktop or Codex.

[Tunnel proof](phase7-diagnostics-20260922.tunnel.txt), exit 0: pinned official
tunnel-client v0.0.14 connects to a **local mock provider**, routes to authenticated
MCP and reads the same primary worker/value. Provider disconnect/reconnect,
MCP replacement and tunnel process recovery leave the local worker alive;
disable/shutdown reclaim owned services. The proof's final caveat describes
that individual command, not the separate Host/identity proofs linked above.
No real provider key or public endpoint was used.

## Regression and scope

[Complete Windows/WSL gate](phase7-diagnostics-20260922.gate.txt), exit 0:
104 Windows / 241 Linux unique Rust tests; Python 167 passed + 8 platform skips
on Windows and 175 passed on Linux. Real uv/IPython/Notebook/MCP acceptance
commands pass. Source identity before/after Linux checks covers 170 files;
[SHA256 manifest](phase7-diagnostics-20260922.sources.json). The copied WSL ledger
is an earlier checkpoint; ledger validation checks arithmetic only, while source
identity covers the implementation/scripts/protocol under test.

P7-06 is complete for the implemented credential boundaries and diagnostic API.
P7-A-02 is complete for actual local compute surviving the real tunnel process's
provider outage and recovery. Neither result proves actual Web ChatGPT access,
installed managed WSL automount/interop, the future diagnostic ZIP, Desktop,
installation/update/repair or JAX GPU computation. These remain explicit gates.
