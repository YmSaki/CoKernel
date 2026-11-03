# CoKernel v1 real-client connection runbook

Status: Phase 7 real connection, updated 2026-09-23. Actual Codex/Web ChatGPT
shared-Session reads pass; actual-client execution remains open. See the
[observed checkpoint](evidence/phase7-real-client-20260922.md). Component smoke
tests alone do not establish account access.

## Prerequisites

The existing Windows Host must be running with a validated Runtime configuration,
an authenticated CoKernel MCP service and the pinned tunnel-client payload.
Use the managed workload identity policy for credential isolation. Development
fixtures under one Linux UID must not be used with real provider credentials.
The installer and full Desktop setup flow are still pending.

Create/select a tunnel in [OpenAI Platform tunnel settings](https://platform.openai.com/settings/organization/tunnels).
Verify its association with the target ChatGPT workspace and the operator's tunnel
permissions. The [official Secure MCP Tunnel guide](https://developers.openai.com/api/docs/guides/secure-mcp-tunnels)
describes the separate Platform and ChatGPT access requirements. CoKernel does
not create an account, grant permissions or infer them from local readiness.
Never paste the runtime key into an AI conversation or command arguments.

## Save and connect

Run the Windows Host binary as a client of the already-running Host:

```powershell
.\cokernel-host.exe remote configure
```

In the native Windows dialog, enter the Tunnel ID (`tunnel_...`) in the user-name
field and the tunnel runtime API key in the password field. These are provider
credentials, not Windows sign-in credentials. The native input path accepts
16–255 graphic ASCII characters for the key. It rejects the OS dialog's
256-character boundary so a clipped paste is not silently saved. The underlying
Host API accepts up to 2048 bytes; full-range Desktop input is still required.

Cancel leaves stored configuration and enabled intent unchanged. Saving uses
Host's existing owner and Windows Credential Manager. An initially disabled
connection stays disabled; replacing credentials on an enabled connection applies
them to that connection. Existing credentials are never displayed or prefilled.
Commands return metadata only. Then inspect and explicitly enable:

```powershell
.\cokernel-host.exe remote status
.\cokernel-host.exe remote start
.\cokernel-host.exe remote status
```

`WAITING_FOR_RUNTIME` means local Runtime is stopped/unavailable; enabling remote
access does not override an explicit Runtime stop. `CONNECTED` establishes
provider polling, not successful tool discovery or workspace authorization.
If a request's completion is uncertain, inspect status/configuration before
retrying. These commands do not spawn an independent Host, Runtime or worker.

Maintenance uses the same owner operations:

```powershell
.\cokernel-host.exe remote stop
.\cokernel-host.exe remote restart
.\cokernel-host.exe remote clear
```

Stop disables remote access while preserving credentials. Clear disables and
confirms tunnel cleanup before deleting credentials. Neither intentionally stops
local notebook execution. Restart reconnects Remote Access; it does not restart
a Notebook Session. No command accepts a key, file path or endpoint override.

## Actual-client proof

1. Create a disposable Project and notebook through the human domain path; execute
   `secret_from_master = 123456789`. Record Project, Notebook, primary Session ID
   and worker generation without any connection keys.
2. In ChatGPT's developer-mode app connection UI, select Tunnel and the configured
   ID, following the official guide. Confirm the actual CoKernel tool list.
3. From an actual ChatGPT conversation, call `get_variable` for that notebook's
   primary Session. Record the returned value and unchanged Session/generation.
4. Connect actual Codex to CoKernel MCP through its supported configuration path;
   repeat the read and execute an existing revisioned notebook cell. A shell HTTP
   client or a test client with origin=AI is not Codex evidence.
5. While that Session contains live variables, stop/reconnect Remote Access and
   perform another local execution. Record unchanged Session/generation/value.
6. Repeat the sharing workflow from the finished Desktop in final acceptance.

Keep evidence specific about which client actually performed each operation.
Do not include real keys, private notebook content or raw credential-bearing logs.
This runbook alone is not acceptance evidence. The linked checkpoint records
native credential submission and actual external calls; it does not close the
full Desktop/installed-system gate.

If a client refuses execution before returning an operation ID, preserve the
reported reason and inspect the document/Session read-only. Do not blindly retry,
mislabel execution as read-only, or route around a client safety rejection. The
2026-09-23 Web ChatGPT attempt returned a generic safety rejection without approval
UI. On 2026-09-24 the user's own Web ChatGPT verification succeeded, and actual
Codex execution also succeeded. See [execution evidence](evidence/phase7-execution-20260924.md).
The previous rejection is historical; final Desktop-origin sharing remains open.
