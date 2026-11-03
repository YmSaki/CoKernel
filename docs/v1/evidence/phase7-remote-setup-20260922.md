# Phase 7 human connection setup — 2026-09-22

Base `f35ca99711a8d6c102ef9e47b857e0e377f87cbb` plus working-tree changes.
P7-A-01 preparation only; actual external-client acceptance remains IN_PROGRESS.

## Implemented path

`cokernel-host remote configure` first authenticates the existing current-user
Host pipe, then opens Windows generic credential UI on a blocking client thread.
It does not open UI inside the server request owner, read old credentials, persist
through the dialog, validate Windows logon or start a second Host/Runtime.
Cancel changes neither configuration nor enabled intent.

The typed PipeClient adapter borrows key bytes directly for serialization into
a zeroizing framed request. It uses existing Host configuration/intent ownership
and response correlation. Exact confirmation returns only configured/tunnel ID;
arbitrary error text, unexpected fields, false status or a mismatched ID cannot
be echoed as a successful receipt. Generic read/write frame buffers are now wiped
on drop too, with the existing documented OS/serde-copy limitations.

`remote status|start|stop|restart|clear` expose existing owner operations. Extra
arguments are rejected before connecting or opening input, with no value echo.
There is no new IPC/MCP method, credential file or domain state machine.

## Tests and limits

Native Windows pack/unpack tests exercise actual credential API blobs, valid keys,
invalid ASCII/domain/length inputs and non-truncating rejection. The first test
attempt demonstrated that an API-valid 2048-byte key is not accepted by Windows
generic credential packing (OS error 50); the native dialog cannot be claimed to
support the Host API's entire range. The Windows SDK `wincred.h` defines the UI
password bound as 256 UTF-16 units. This adapter accepts 16–255 graphic ASCII
characters and rejects the boundary itself, including a potentially clipped
paste. The 2048-byte domain API bound is unchanged; full-range Desktop input
remains required. Tests cover 255 preservation and 256 rejection.

Cancellation/native-error result handling yields no credentials. A malicious
receipt corpus verifies private strings are excluded from fixed setup errors.
The native Named Pipe/Credential Manager round trip now uses the typed configure
adapter, verifies persistence and clear, checks no key in response/events/files,
and proves saving did not start Runtime. CLI rejection is exercised against the
actual Host executable for all six actions.

The initial sandbox-only unit run could not access the current-user registry or
Credential Manager: [recorded failure](phase7-remote-setup-20260922.unit.txt).
Its new input/receipt tests passed. Required native API tests were then run with
the actual current-user environment, without skipping failures.

[Complete Windows/WSL regression](phase7-remote-setup-20260922.gate.txt), exit 0:
108 Windows / 241 Linux unique Rust tests; Python 167 passed + 8 platform skips
on Windows and 175 passed on Linux. Real uv/IPython/Notebook/MCP smokes pass.
Byte identity before/after Linux verification covers 171 files;
[SHA256 source manifest](phase7-remote-setup-20260922.sources.json).
WBS arithmetic and git diff whitespace checks pass.

No native dialog was displayed or visually exercised in this checkpoint. No
provider key, real tunnel, actual Codex/Web ChatGPT connection or Desktop proof
was created. Full provider/workspace acceptance remains required. The
[connection runbook](../REMOTE_CONNECTION.md) describes setup and actual-client
evidence without asking for keys in chat.

## Next prerequisite

The [real-connection prerequisite audit](phase7-real-connection-prerequisite-audit-20260922.md)
finds a schedule dependency omission: real credentials need the dedicated WSL
drive/interop boundary before Phase 7 acceptance. D-028 assigns minimal reusable
bootstrap provisioning/validation to that consumer; full installer UX remains
Phase 9. No existing development/legacy distro will be repurposed or reset.

API basis: Microsoft documents that
[CredUIPromptForWindowsCredentialsW](https://learn.microsoft.com/en-us/windows/win32/api/wincred/nf-wincred-creduipromptforwindowscredentialsw)
does not persist credentials and requires clearing/freeing its output blob;
[CredUnPackAuthenticationBufferW](https://learn.microsoft.com/en-us/windows/win32/api/wincred/nf-wincred-credunpackauthenticationbufferw)
decodes it. Actual external setup follows the
[OpenAI Secure MCP Tunnel guide](https://developers.openai.com/api/docs/guides/secure-mcp-tunnels).
