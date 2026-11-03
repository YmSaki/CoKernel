# Phase 7 Host credential handoff — 2026-09-22

Base: `f35ca99711a8d6c102ef9e47b857e0e377f87cbb` plus working-tree changes.
P7-02 controlled-handoff checkpoint: native component tests, actual Host/WSL proof
and final complete Windows/WSL gate PASS. P7-02 is locally DONE. No real provider credential,
external tunnel object or public inbound listener was used.

## Ownership and lifecycle

The Windows Host has one bounded Remote Access owner for configuration, durable
remote intent and bridge handoff. Windows Credential Manager remains credential
truth. Atomic `remote-access.json` contains only schema and enabled intent; Runtime
desired RUNNING/STOPPED remains independent. Remote enable never starts a stopped
Runtime. An unchanged desired target does not reload/retransmit keys; changed
configuration or Runtime generation reads the current Windows credential.

The owner serializes configuration, enable/disable/reconnect and clear. A reply
disappearing does not cancel accepted work. Clear persists disabled intent, confirms
cleanup on a live bridge, then removes the credential; uncertain cleanup preserves
the credential and returns a fixed error. Host exit drains the remote owner before
releasing its state lock. A new Host restores both independent desired states.

Public state is a safe projection with a loading/unknown initial intent. Handoff
acknowledgements and observations are validated, correlated and generation-bound.
CONNECTED without a live PID and DISABLED with a PID are invalid. Arbitrary Runtime
errors/results from credential operations are never returned to Desktop. Owned
bridge frame buffers and the extracted JSON key are zeroized after encoding/use;
this does not claim complete erasure of every JSON/OS/transport copy.

See INTERFACE_CONTRACTS sections 32–34 and MCP_TUNNEL sections 20–21.

## Failures found and corrected

The first full gate failed the existing
`runtime_events_follow_bridge_generation_and_slow_subscribers_do_not_block_calls`
test: a Host-owned Remote Access event entered the Runtime event stream with
generation zero. The existing Runtime sequence/generation contract was preserved.
Remote Access now has its own state watch and Named Pipe event branch; it is included
in the initial Host snapshot without impersonating a Runtime event. The unchanged
regression test passes. [Initial failing gate](phase7-host-handoff-20260922.gate-before.txt).

The second gate exposed a cancellation-test precondition: Remote Access now has a
legitimate asynchronous LOADING -> DISABLED startup event. The initial snapshot
now consumes the exact remote watch version it contains, avoiding duplicate
notification. The test waits for that documented startup event when necessary,
then still requires cancellation of a pending read to poison the client and require
reconnection. All 17 Named Pipe tests pass. The previous failure is retained in
[the event-read log](phase7-host-handoff-20260922.gate-event-read-before.txt).

The first native Host/WSL execution passed, but its independent process scanner
looked for the wrong worker executable spelling (`cokernel_worker` instead of
`cokernel-worker`). Its assertion failed and no isolation proof was accepted from
that attempt. The scanner was corrected and the complete native proof rerun in a
fresh disposable fixture; final evidence is below.

## Verification

- Native Windows tests use real Credential Manager and Named Pipe with a framed
  mock Runtime. They prove enable while stopped, identical enable without duplicate
  handoff, rotation, Runtime crash/restart, Host restart using the latest credential,
  explicit reconnect/disable/clear, safe errors and secret-free local state files.
- A pending tunnel stop makes clear fail while retaining the credential and
  persisted disabled intent. Independent Host control remains responsive.
- An invalid CONNECTED observation opens a bounded protocol-error circuit;
  explicit reconnect recovers after the fixture is corrected.
- A portable intent-store test proves default, persistence, bounds and corrupt
  state rejection; it never infers intent from credential presence.

Actual fixture: `/home/ymsaki/.cache/cokernel-v1-dev/host-proof-c189baf9ab80`;
native state `target/dev-tools/host-handoff-state-3`. The pinned tunnel-client
v0.0.14 connects only to a loopback mock provider, using a synthetic runtime key
configured through native Host IPC and Windows Credential Manager.

| Proof | Evidence |
|---|---|
| Actual Windows Host -> WSL -> tunnel, CONNECTED, same worker/value, clear after stop | [Native log](phase7-host-handoff-20260922.host.txt), exit 0 |
| Independent live environment/argv checks and final child cleanup | [Postconditions](phase7-host-handoff-20260922.postconditions.json), worker/launcher 4771/4801, tunnel 4923, exit 0 |
| Imported Notebook validity, outputs/attachment/shutdown edit, failed import absent, worker gone | [Notebook validator](phase7-host-handoff-20260922.notebook.txt), exit 0 |
| Final complete canonical Windows/WSL gate | [Gate log](phase7-host-handoff-20260922.gate.txt), exit 0; Windows 102 / Linux 236 unique Rust tests; Python 166 + 8 platform skips / 174 passed |
| Source identity before and after Linux verification | 164 files match; [SHA256 manifest](phase7-host-handoff-20260922.sources.json) |

The live scanner confirms synthetic provider key/local bearer absent from worker
environment/argv, present in the intended tunnel environment and absent from its
argv. The final Host log and state/notebook files contain no synthetic provider key.
The synthetic Windows credential was cleared through Host IPC and has an isolated
RAII cleanup guard. No unrelated credential was enumerated or modified.

## Remaining gates

This proves the controlled handoff path and local lifecycle. The development distro
still uses one Linux UID, so it does not prove that hostile notebook code cannot
read another same-UID process or the control-plane filesystem. Managed workload
identity isolation, complete diagnostics/redaction, actual Codex/Web ChatGPT,
Desktop and JAX GPU acceptance remain required. P7-GATE and the full goal remain open.
