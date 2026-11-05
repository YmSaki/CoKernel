# Release candidate evidence — 2026-11-04 JST

Code under test: `57f7ae96636453fd3b15181fa9ff52ca70ed9902`.
[Build run](https://github.com/YmSaki/CoKernel/actions/runs/41735719116).
[Implementation PR](https://github.com/YmSaki/CoKernel/pull/46).

## Confirmed Linux results

The release Linux job (126264269305) passed the complete `scripts/v1/check.sh`,
release-image assembly and `smoke_tunnel_supervision.py` against the pinned actual
tunnel-client v0.0.14. Source diagnostics used the locked Worker environment.

The gate includes Rust fmt/check/Clippy/tests, Python Worker tests, actual uv
Project/package operations, actual IPython child execution, notebook atomic
persistence and revision conflicts, Session isolation/FIFO/interruption/restart,
MCP authentication and bounded inputs/results, and Runtime-owned recovery.
The HTTP authoring proof creates and resynchronizes a uv Project and creates a notebook, inserts and updates a
cell, rejects stale edits, saves, executes and checks PERSISTED/SUCCEEDED and the
retained value. It rejects path escape and package-option injection.

The real tunnel binary was exercised with a local synthetic provider. Requests
reached the actual authenticated MCP server and primary Python worker; provider
outage/recovery and MCP replacement retained the worker/value. This is not a
proof of an external OpenAI connector session.

Image identity: `b6ac5795-1cdd-4d51-9a3b-f4d5f2bef36a`.
Image SHA256: `80c96ce9712fbcb250bc96edb2fe2f7a53a7159b5fc0cc6e18977a69c6a8b5bc`.

## Windows and final acceptance

Native Windows job 126267038122 passed the complete Host/bootstrap/Desktop test
selection, including actual Windows Credential Manager round-trip, Named Pipe
ACL/peer checks, reconnect/lifetime behavior and detached launch flag parsing.
It also passed TypeScript/Vite build, optimized Rust build, PowerShell parsing,
Microsoft WebView2 signature verification and Inno Setup compilation. Windows
fixtures run sequentially to avoid unrelated cold interpreter starts competing
against short test deadlines; concurrency within each scenario remains tested.

Draft prerelease: [candidate.7](https://github.com/YmSaki/CoKernel/releases/tag/untagged-9f5428528316218c3b2e).
`CoKernelSetup.exe`: 72,860,275 bytes.
Installer SHA256: `ad7b7b477a494e5b8d2c699d734b3226e732246b6a4c7b33ce9db8899fab1b88`.
`SHA256SUMS.txt` is attached alongside the installer.
This is an unsigned, fresh-install-only candidate; no installed-runtime migration
or destructive uninstall of notebook/credential state is claimed.

No fresh installed-system, reboot-resume, actual NVIDIA computation or external
ChatGPT call on this packaged commit has been observed from the development
host. Prior branch evidence does not close these gates. Production publication
and the Phase 11 gate remain open until the required evidence exists.

The final Host CLI permits explicit `--start --background` together so first
setup does not attach the Host to the installer console. Unit tests cover the
combined flags, persisted login intent and conflicting/duplicate flag rejection.
The actual installer-close lifecycle still requires installed-system acceptance.

The final read-only probe through the connected CoKernel app returned
`tunnel_client_not_seen` (the tunnel client had not been seen for 300 seconds).
Consequently no live-user-connector or actual GPU execution is claimed. The
Windows install and native credential/connector setup must precede that proof.
