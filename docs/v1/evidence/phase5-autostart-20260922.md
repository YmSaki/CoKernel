# Phase 5 current-user autostart — 2026-09-22

Base: `f35ca99711a8d6c102ef9e47b857e0e377f87cbb` plus the working tree.
P5-08 is locally DONE for its Host registration/configuration and independent
lifetime criterion. Phase 5, installer integration and real Windows sign-in
acceptance remain open. No commit, PR or actual Windows Run registration was made.

## Implementation and scope

The Windows Host owns one HKCU Run value and exposes strict get/set settings over
the authenticated current-user Named Pipe. The CLI provides status/enable/disable
for installation/repair. Clients supply only a boolean; executable, configuration
and state paths come from the trusted Host launch. Commands use absolute quoted
Unicode paths without a shell, reject control/quote characters and enforce the
260 UTF-16 unit command limit. Enable validates the existing executable/config and
bounded WSL configuration before writing; disable is idempotent and deletes only
the product value. Registry work uses the blocking pool within the IPC request bound.

Registration status distinguishes exact launch matching from effective Windows
startup permission. When registered, effective enablement is unknown because
Startup Apps/policy can suppress execution. Registry/config errors fail explicitly.
Changing registration never changes Runtime desired state or live Session memory.

`serve ... --background` runs the Windows subsystem Host without console attachment
or console-interrupt dependency, reads persisted desired state and requires no
Desktop client. The registration command does not force RUNNING. Foreground CLI
keeps redirected output or attaches the parent's console where available.

## Verification

| Proof | Result |
|---|---|
| Windows registry round-trip/idempotence, unchanged desired bytes | PASS, UUID test key only |
| Windows argument parser with Unicode/spaces/metacharacters/trailing separator | PASS |
| Foreign value preservation, mismatched target, malformed/type/oversize rejection | PASS |
| Invalid/overlong startup command does not register | PASS |
| Named Pipe rejects command/path injection and missing trusted launch binding | PASS |
| Actual Host executable, default STOPPED, persisted RUNNING, explicit stop, later STOPPED | PASS, real WSL, no Desktop or startup registration |
| All clients disconnect/reconnect without replacing background Host/Runtime | PASS |
| Full check.ps1 including actual WSL check.sh | PASS, exit 0 |
| Native Rust | 81 unique tests; fmt/check/clippy PASS |
| Linux Rust | 197 unique tests; fmt/check/clippy PASS |
| Native Python worker | 163 PASS, eight Unix-only skips |
| Linux Python worker | 171 PASS |
| Windows/WSL source identity before and after gate | 129 files match |
| Actual Named Pipe -> Host -> WSL -> uv/IPython notebook proof | PASS, exit 0 |
| Independent nbformat/output/attachment/shutdown edit/worker exit check | PASS, exit 0 |

Nested credential subprocess tests are counted once. The background proof launches
the compiled product executable with closed stdio and no console, using fresh state
directories. It refuses an already-occupied default Host pipe. The RUNNING case
establishes generation 1 with a real Runtime; STOPPED cases remain generation 0.
It explicitly stops Runtime before terminating only its own disposable Host.

Development WSL root: `/home/ymsaki/.cache/cokernel-v1-dev/host-proof-8e4811b741a2`.
Native states: `target/dev-tools/host-autostart-state` and
`target/dev-tools/host-autostart-pipe-state`. The actual pipe proof also verifies
primary Session sharing, accepted execution after disconnect, async Project/package
completion, control during inspection, chunked Windows import and saved outputs.

## Failures found and resolved during verification

The initial default-sandbox registry test could not open its disposable test key.
The same test passed with the permitted registry access; no production Run value
was modified. One whole-gate attempt overlapped execution of the background example
with relinking that executable and failed with Windows LNK1104. Running verification
sequentially after the example exited resolved that file lock.

A later gate exposed a pre-existing deferred-request fixture defect: its launcher
used Project name `blocked`, but the fake worker only served cooperative requests
for `healthy`/lifecycle/execute names. Once startup was released, it exited after
handshake as an unsupported mode. Depending on timing, restart saw CRASHED rather
than STOPPED. An initial inspection probe reproduced early disconnection; inspection
is itself unsupported by this minimal fake worker, so the final regression instead
requires successful captured cell execution before racing restart against stop.
The fixture now serves `blocked` cooperatively after its launcher releases it.
The strengthened test and the entire final gate pass; production Session state
handling was not weakened to accept the incorrect crash.

## Evidence limits and follow-up

Actual Windows sign-out/sign-in was not performed. P11-L6-13 remains open, as do
installer packaging, Desktop, real Codex/Web ChatGPT MCP, managed WSL isolation
and JAX GPU computation. Development Ubuntu and Rust caller-origin markers do not
close those acceptance criteria. Metrics (P5-09) are the next Phase 5 work.

Artifacts: `phase5-autostart-20260922.gate.txt`, `.background.txt`, `.real.txt`,
`.postconditions.txt`, `.sources.json`, `.link-before.txt`, `.fixture-before.txt`
and `.fixture-probe.txt`. Final ledger closure follows the gate; its recorded ledger
check therefore contains the earlier 104-DONE/one-IN_PROGRESS count.
