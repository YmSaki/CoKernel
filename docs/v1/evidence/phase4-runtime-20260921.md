# Phase 4 Runtime notebook acceptance — 2026-09-21

Base: `f35ca99711a8d6c102ef9e47b857e0e377f87cbb` plus the current working tree.
No commit, pull request or GitHub issue closure is implied by this local gate.

## Result and scope

Phase 4's local gate passes: a standard imported notebook executes in its uv
Project's real IPython Session, output persists through Notebook Document Service,
and an independent nbformat 5.11.1 validator accepts the saved file. This closes
P4-08, P4-13, P4-12, P4-A-01 and P4-GATE locally.

The canonical combined Windows `scripts/v1/check.ps1` and real WSL
`scripts/v1/check.sh` completed with exit 0. The developer dispatcher uses the
native Linux copy to avoid /mnt/c interpreter latency; it invokes the actual
canonical scripts and checks 95 source/fixture/lock files by SHA-256 before and
after the Linux run. It is test tooling, not the production Windows Host.

| Verification | Evidence |
|---|---|
| Windows fmt/check/clippy/workspace tests | PASS; 39 Rust tests, including the Named Pipe feasibility test |
| Windows script behavior/contract tests | 21 PASS, including native failure injection |
| Windows worker pytest | 163 PASS, 4 Unix-only skips |
| WSL fmt/check/clippy/workspace tests | PASS; 133 Rust tests (2 domain, 3 MCP scaffold, 4 protocol, 83 Runtime unit, 7 notebook model, 6 document service, 28 Session process) |
| WSL worker pytest | 167 PASS, no skips |
| WSL script tests | 19 PASS, 2 Windows behavioral tests skipped and covered above |
| Project/uv, worker transport, overlay, SIGINT | PASS |
| Real Session Supervisor/captured outputs and hostile inspection | PASS |
| New framed notebook Runtime smoke | PASS |

The actual RPC smoke creates disposable Projects, installs a local package into
the selected Project, imports the compatibility fixture whose kernelspec names a
different environment, and executes authoritative cell IDs. It demonstrates
package result 42, persistent `x=123` then `x+1=124`, stdout, HTML output and
`secret_from_master=123456789` through safe inspection. Raw JSON checks preserve
unknown notebook metadata and Markdown attachments/source shape. Dirty edits are
flushed on close; reopening retains NotebookId and the same primary Session.
Package removal marks the live Session stale while its existing variable remains.

## Conflict and boundary evidence

- Human/MCP-origin coordinator calls use the same Session queue. Two accepted
  cells at one document revision both persist without false output conflicts.
  These origins are simulated; no actual Desktop or MCP client is claimed.
- Dropping the requesting coordinator clone does not cancel the Runtime-owned
  result/persistence task. Closing a view does not remove document identity.
- Other-cell edits do not invalidate execution. Target source edits including
  ABA, deletion/reinsertion, explicit reload and external file changes reject
  old results. A deterministic unit test commits a newer same-cell result first
  and proves old/duplicate completions cannot replace it.
- Wrong Project/Notebook/Session/cell/revision admission is rejected. Public RPC
  rejects supplied source or origin. Revision errors include current_revision.
- Invalid/oversized base64 aborts the owner's import; a wrong-Project call cannot
  cancel it. Failed imports do not publish a target file.
- A 9 MiB notebook imports successfully. Full read returns a bounded limit error,
  chunk read succeeds and the bridge remains responsive. Wrong read revision is
  rejected. Live-Session Project removal is rejected.

The first new smoke failed because nbformat normalizes list-valued sources when
reading. The assertion now compares saved raw JSON separately from independent
format validation; product source preservation was intact. Final full gate passes.
The deliberate crash smoke emits a benign no-such-process kill diagnostic;
its crash/survivor assertions and final acceptance both pass.

## Final protocol fixture follow-up

Review after the full gate found the existing execute-cell example omitted the
now-required project_id. The fixture was corrected and a typed protocol test now
parses it, rejects every missing required field, and rejects injected source and
origin. Only that fixture and a test in methods.rs changed after the full gate.
Native Windows and WSL protocol tests (5 each) and clippy pass; the native fixture
validator also passes. Final 95-file source identity is rechecked. Product Runtime
code is unchanged from the full gate; the source manifest records this final state.

## Reproduction and limitations

Windows Rust/Cargo 1.91.1, uv 0.12.11, Python 3.11.9; worker Python 3.12.14.
WSL Ubuntu Rust/Cargo 1.88.0, uv 0.12.11, Python 3.12.14, IPython 9.17.1.
The [dispatcher](phase4-runtime-20260921.runner.ps1) records the exact command,
PATH and cache settings. The [comparison helper](phase4-runtime-20260921.compare.py)
uses target/dev-tools/snapshot-files.txt; recreate that inventory from the keys of
the [source manifest](phase4-runtime-20260921.sources.json). Evidence and subsequent
documentation/ledger updates are excluded from the executable-source inventory.

This developer Ubuntu environment is not the hardened installed CoKernel distro.
Windows import streaming, Host/UI lifecycle, forwarded events, authentication,
actual Codex and web ChatGPT, Desktop and JAX GPU remain later acceptance work.
The current bridge dispatches requests serially; package operations are synchronous.
Operation status is polled and Runtime-lifetime. Filesystem conflict detection has
the documented optimistic check-to-rename window against external writers.
No broader security/installation/release gate is closed by this evidence.

Logs: [combined canonical gate](phase4-runtime-20260921.output.txt),
[Windows protocol follow-up](phase4-runtime-20260921.protocol-windows.txt),
[WSL protocol follow-up](phase4-runtime-20260921.protocol-linux.txt).
