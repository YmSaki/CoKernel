# Phase 7 Windows Secret Store — 2026-09-22

Base: `f35ca99711a8d6c102ef9e47b857e0e377f87cbb` plus working-tree changes.
P7-01 is locally DONE. Controlled Host-to-Runtime secret handoff, tunnel
supervision, actual remote client connection and Phase 7 acceptance remain open.
No real external credentials were requested in chat, loaded, migrated or used.
Tests created and removed only uniquely scoped disposable test credentials.

## Implementation and authority

`cokernel-host::secrets` supplies a synchronous backend abstraction and one bounded
blocking owner. Windows uses generic Credential Manager entries with
CRED_PERSIST_LOCAL_MACHINE: persistence belongs to the current user on this PC,
not every machine user. This follows Microsoft's
[CREDENTIALW contract](https://learn.microsoft.com/en-us/windows/win32/api/wincred/ns-wincred-credentialw)
and [CredWriteW semantics](https://learn.microsoft.com/en-us/windows/win32/api/wincred/nf-wincred-credwritew).
An atomic versioned blob pairs the tunnel ID with its runtime key. The target is
scoped to the canonical Host state directory; no credential enumeration occurs.

Host IPC offers configure, get_configuration and clear_configuration. It exposes
only configured/tunnel_id, accepts no caller-selected storage target, and never
returns key bytes. These methods are absent from Runtime/MCP transports. They do
not start Runtime or activate remote access. The queue serializes eight waiting
operations plus one active operation without blocking other Host control work.
An accepted write survives dropped response delivery. Failed backend mutation
preserves the previous credential; malformed data requires explicit repair.

Secret types deliberately lack Debug/Serialize/Clone. Owned key/encoding buffers
use zeroize 1.9.0 (locked; MSRV 1.85, verified on WSL Rust 1.88). Native read buffers
are wiped before CredFree. This is application-buffer hygiene, not a promise to
erase every transport/JSON/OS copy. No persistent plaintext file is written.
Exact schema, bounds, errors and lifecycle limits are in INTERFACE_CONTRACTS §32.

## Verification

The actual Windows API test saves, reads through a reopened store, replaces and
clears a disposable credential. A second state scope remains empty. State files
remain empty, and clear is idempotent. The actual Named Pipe test configures,
disconnects/reconnects, reads only metadata, independently verifies the stored key,
rejects unknown fields/invalid IDs/null keys/secret-read requests, and confirms
invalid mutation did not replace existing data. Success/error/snapshot responses
and Host state files contain no key. Runtime was never started by configuration.
RAII cleanup removes only the test target even when the test fails.

Three portable tests cover bounded/versioned encoding, static non-echoing errors,
failed mutation, FIFO, capacity exhaustion and disconnected-reply ownership.
The complete canonical gate reran the existing Notebook/Session/Host/MCP suite.

| Check | Evidence |
|---|---|
| Actual check.ps1 + actual WSL check.sh | [Final full log](phase7-secret-store-20260922.gate.txt), exit 0 |
| Rust fmt/check/clippy/workspace | Windows 98 and Linux 228 unique passing test names |
| Python worker | Windows 166 PASS + 8 Unix-only skips; Linux 174 PASS |
| Source identity before/after Linux gate | 155 byte-identical files; [SHA-256 manifest](phase7-secret-store-20260922.sources.json) |
| Native Credential Manager and Named Pipe | Four secret unit tests and one pipe test PASS within native gate |
| Existing actual uv/IPython/MCP supervision | All canonical smokes PASS |

## Next tunnel dependency preparation

The existing v1 intent retains OpenAI Secure MCP Tunnel. The current official
[guide](https://developers.openai.com/api/docs/guides/secure-mcp-tunnels) confirms
outbound private MCP connectivity and separate tunnel/workspace permissions.
No public proxy or Cloudflare deployment was introduced.

The official [v0.0.14 release](https://github.com/openai/tunnel-client/releases/tag/v0.0.14)
was inspected. Its Linux amd64 archive was downloaded only to development storage
and verified against the release API's published SHA-256:
`15bd17e805cad39d412199115bb9e10a978dd35258a114cdf25dd2ae6681c7d3`.
The binary reports `0.0.14+0f870e50a973fa820d4c409000059e181e8d242b`.
Version, quickstart help and run help all exited 0 with a cleared environment and
no credentials. [Captured help](phase7-tunnel-client-20260922.help.txt) establishes
support for secret references, MCP runtime/discovery headers, localhost/Unix health
and readiness options. No tunnel daemon was launched or tunnel control-plane request sent.

Development payload is at
`/home/ymsaki/.cache/cokernel-v1-dev/tunnel-client-v0.0.14`; the archive and release
metadata are in `target/dev-tools/`. This is dependency preparation, not a finished
installer pin/attestation policy or successful tunnel connection.

P7-02 must deliver credentials only to the required controlled tunnel component;
P7-03 onward must supervise the actual client, reconcile changes, enforce MCP-first
readiness and demonstrate disconnect survival. The current Windows store does not
prove managed Linux identity separation, actual Desktop/Codex/Web ChatGPT access,
or JAX GPU acceptance. The full user goal remains active.
