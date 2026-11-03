# Phase 7 tunnel owner and state-model audit — 2026-09-22

Base: `f35ca99711a8d6c102ef9e47b857e0e377f87cbb` plus working-tree changes.
Scope: Runtime-owned tunnel lifecycle, private launch/health adapter and actual
pinned-client integration against a local mock provider. This does not prove
Windows Secret Store handoff, managed Linux identities or real Web ChatGPT access.
No real provider keys, external tunnel objects or public listeners were used.

## State-model audit and resolution

Authority: user requirement for coherent normal state transitions, AGENTS ownership
invariants, MCP_TUNNEL sections 13–17, then the Runtime implementation. This is a
targeted lifecycle/state-mapping audit using upstream-contract-audit; it does not
claim to audit every existing Session, Host or installer transition.

The first unaccepted implementation draft assigned public state in several loop
branches, reset the label to STARTING on iteration, and gave every repeated enable
a new configuration epoch. Those choices made public state an input to lifecycle
decisions and made unchanged user intent restart a child. The accepted lifecycle
contract did not specify this behavior; the earliest contract gap was the missing
normal reconciliation/observation distinction in MCP_TUNNEL section 13
(`UNDERSPECIFIED`, STATE_MACHINE/STATE_MAPPING). The draft was replaced before
acceptance; section 21 and INTERFACE_CONTRACTS section 33 now close that gap.

Normal control has one rule, implemented in `tunnel/model.rs`: compare desired
incarnation `(configuration epoch, MCP generation, endpoint)` with the owned child.
Equal means observe, different means reclaim, absent child plus desired target
means launch, and absent target means wait. Shutdown first requires no owned child.
There are no cause-specific restart paths for disable, configuration replacement
or MCP replacement. Asynchronous preparation is revalidated against the same target
before launch. Same configuration enable and disabled stop are idempotent; explicit
restart is distinct. Events are emitted only for changed snapshots.

Minimal counterexamples covered by the implementation/tests include: repeated
enable must retain PID; replaced MCP must invalidate the old target even if its
endpoint is reused; stopped MCP must prevent tunnel launch; disable must reclaim
before reporting DISABLED; an exhausted retry circuit must not be bypassed by an
unchanged enable; `/readyz` success with no successful provider poll must never
report CONNECTED. Error handling separately owns retry allowance/deadlines and
retains child ownership when cleanup is uncertain. Old-incarnation failures remain
evidence without consuming a newer configuration's retry allowance.

## Implementation

- Runtime owner/task and trusted bridge controls; no worker or document ownership.
- Optional bootstrap configuration and private per-attempt Unix health socket.
- Clear launch environment, env-reference argv, narrow provider/MCP credential
  handoff, zeroizing owned buffers, no raw process output in events/logs.
- MCP-before-tunnel dependency and endpoint-generation reconciliation.
- Exact readiness plus successful-poll freshness, local degraded/reconnect state,
  finite retry budget, explicit restart, cleanup before shutdown acknowledgement.
- Shared child-group cleanup primitive with existing MCP supervision.

The adapter contract was verified against official pinned v0.0.14
[health implementation](https://github.com/openai/tunnel-client/blob/v0.0.14/pkg/runtimehealth/health.go),
[poll metrics](https://github.com/openai/tunnel-client/blob/v0.0.14/pkg/controlplane/internal/metrics.go),
and [provider protocol](https://github.com/openai/tunnel-client/blob/v0.0.14/docs/protocol.md).
Readiness alone permits authentication/timeout fallbacks in that client; this
product requires exact `ready` and a fresh successful-poll timestamp instead.
Observed real metric samples use scientific notation, handled with finite integral
value/bounds validation. Provider HTTP/transport/diagnostic details are not copied
into the public state.

## Verification

P7-03..05 are locally DONE. The complete canonical gate and final-source actual
pinned-client component proof both exited 0.

| Verification | Evidence |
|---|---|
| Windows check.ps1 + actual WSL check.sh | [Full log](phase7-tunnel-supervision-20260922.gate.txt), exit 0 |
| Rust fmt/check/clippy/tests | Windows 98 and Linux 235 unique passing test names |
| Python worker | Windows 166 PASS + 8 Unix-only skips; Linux 174 PASS |
| Tunnel-focused tests | Seven model/security/real-child tests PASS in the Linux gate |
| Source identity | 162 byte-identical source files before/after Linux gate; [SHA-256 manifest](phase7-tunnel-supervision-20260922.sources.json) |
| Actual pinned client, real MCP and uv/IPython | [Final component log](phase7-tunnel-supervision-20260922.actual.txt), exit 0 |

Binary version was independently rechecked as
`0.0.14+0f870e50a973fa820d4c409000059e181e8d242b`; the official archive's
SHA-256 `15bd17e805cad39d412199115bb9e10a978dd35258a114cdf25dd2ae6681c7d3`
and acquisition evidence are recorded in E-SECRET-STORE-20260922.

An initial focused test failed because the mock MCP served HTTP/1.0 while the existing
MCP health contract requires HTTP/1.1. Correcting the fixture produced seven passing
model/security/real-child tests; no production health contract was weakened.

Both the earlier and final-source actual pinned-client runs passed: synthetic provider credentials, local
control plane -> real MCP -> real uv/IPython primary Session; value 123456789 and
unchanged worker identity; network outage/reconnect while local cells execute;
MCP replacement, tunnel crash recovery, stop/shutdown, denied MCP control and
credential argv/environment/event/stderr checks. The final run followed the successful full gate.

## Remaining acceptance

P7-02 must connect Windows Secret Store to this bridge surface, serialize live
configuration replacement/clear, persist desired Remote Access state and reapply
it after Host/Runtime recovery. The Host configuration methods remain storage-only.
The present same-UID development distro is not worker secret isolation: managed
identities/permissions remain mandatory. Actual Codex/Web ChatGPT/Desktop and JAX
GPU acceptance remain open, and P7-GATE is not closed by a local mock provider.
