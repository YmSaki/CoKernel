# Real connection checkpoint — 2026-09-22

Latest 2026-09-24: **P7-GATE PASS**. The user successfully executed from Web
ChatGPT; actual Codex independently inspected that result and completed its own
execution. See [execution closure](phase7-execution-20260924.md). Final revision
is 4, same primary Session/worker generation 1. The rejection below is historical.

Updated 2026-09-23: P7-A-01's remote same-Session read criterion passes with both
actual Web ChatGPT and Codex. P7-GATE remains IN_PROGRESS: actual-client execution
has not passed. The Web ChatGPT execution attempt was rejected by its safety
check, without a CoKernel operation ID. This is not a Runtime execution failure
or permission to weaken MCP annotations.

## External configuration and live Session

The user logged in to Platform, explicitly authorized tunnel creation, then
entered the runtime key in CoKernel's native credential dialog. Created tunnel:
`tunnel_6ab21d83a06c81919b9fc65cd7facf99`, name `CoKernel v1`, associated with the
current Platform organization and available ChatGPT workspace. Existing tunnels
and the old CoKernel/Jupyter app were left intact. No key was printed or placed
in command arguments. Windows Credential Manager holds the Host-owned key.

The real Host is running against the v3 validation distro (now repaired with
Ubuntu's CA package). State is under
`target/dev-tools/validation-20260922/remote-host`; owned Host PID was 45176 at
creation. No auto-start registration was changed. Do not stop/restart this Host
or its distro while actual client proof is using the live Session.

`shared_session_prepare` connects to that existing Host and creates a disposable
Project/imported Notebook through the same human IPC/domain path. It does not
spawn Runtime/worker itself. Preparation receipt and local source notebook are
in that Host state directory. It leaves the primary Session alive deliberately.
`--inspect` only checks the saved IDs/generation/value/PID against the live Host.

- Project: `629f42c4-beda-4bd8-b6f9-d770c431d724`.
- Notebook: `ee0b41ce-9966-4482-831f-fcdaf31583eb`.
- Primary Session: `e067e0f3-0329-4825-9503-c7809d2351c8`.
- Worker PID 95, generation 1. `secret_from_master` = 123456789.
- `human-seed` executed and persisted, revision 2.
- `ai-check` is `secret_from_master + 1`; still unexecuted at the 2026-09-23
  Codex observation, with revision 2 and no outputs.

[Preparation](phase7-real-client-20260922.prepare.txt),
[CONNECTED state](phase7-real-client-20260922.connected.txt), and
[unchanged Session/value after connection](phase7-real-client-20260922.shared-after-connect.txt)
are actual Windows Host/WSL/provider observations. They are not actual AI calls.

After the user logged in to ChatGPT, `CoKernel v1` was registered and connected:
app `asdk_app_6ab22284fe2c81918cbcbc80e50fde54`, version
`asdk_app_v_6ab22284fe3c8191b68711655e86ccba`. The settings UI displayed all 15 MCP
tools and their schemas/annotations. The connector uses the authorized tunnel;
internal MCP bearer authentication remains enabled. No separate connector OAuth
is added. The [actual Web ChatGPT conversation](https://chatgpt.com/c/6ab222fc-3b00-83ee-a3e3-bab649674426)
read the existing Session and variables without being told their expected values.
An isolated `get_variable` call's expanded tool-result UI returned:

```json
{"name":"secret_from_master","reason":null,"supported":true,"type_module":"builtins","type_name":"int","value":123456789}
```

The tool request contained the Project and primary Session IDs above. The actual
`read_notebook` tool card also returned revision 2, the saved human output and the
unexecuted `ai-check` cell. These are inspected tool responses, not only the
assistant's prose summary. A multi-call tool-card display was ambiguous; the
single-call proof removes that ambiguity without changing backend behavior.

On 2026-09-23, the installed CoKernel connector became available in this actual
Codex task's tool registry. Codex directly called `get_session`, `get_variable`
twice and `read_notebook`; [recorded responses](phase7-real-client-20260923.codex.json)
show the same primary Session, worker generation 1, PID 95 and value 123456789.
The Session is IDLE with an empty queue; the document remains revision 2. No test
HTTP client, alternate Runtime or synthetic caller identity was used for these
four calls. The Windows Host PID 45176 was independently observed still running.
The human seed still came through the Host example, not the unfinished Desktop.
The existing `shared_session_prepare --inspect` binary then passed via the real
current-user Named Pipe (exit 0): same primary worker/value, Runtime generation 1,
tunnel generation 3, CONNECTED, desired enabled, zero consecutive failures. This
is an independent read-only Host observation, not an additional AI execution.

### Actual execution remains open

Web ChatGPT was asked to read the revision and execute the stored `ai-check` cell
exactly once, then query its operation. It reported an immediate OpenAI safety
rejection, with no approval UI, error code, HTTP status or operation ID visible.
Its only returned explanation was:

> このツールの呼び出しは、OpenAI の安全性チェックによってブロックされました。送信内容を再度確認してください。

The independent Codex document read confirms no persisted execution. It does not
by itself prove a server never received a request; the lack of admission is the
ChatGPT client's report. No uncertain execution was retried. No read-only,
destructive or open-world annotation was changed, and the blocked action was not
reissued through another path. The user agreed to check execution directly in
that ChatGPT conversation. Until a successful operation/result is inspected,
actual ChatGPT/Codex execution, P7-GATE and final Desktop sharing remain open.

## Actual-connection discovery: CA roots

Image v3 lacked the OS trust bundle. The local health endpoint said ready while
the successful-poll metric was zero and error count increased; Host correctly
reported DEGRADED. Ubuntu signed package indexes and
`ca-certificates 20260601~24.04.1` were installed only in this owned validation
distro. Reconnecting only the tunnel reached CONNECTED. Runtime/worker generation,
PID and shared value survived; no Session restart was used as recovery.

The reusable image builder now requires a pinned public Ubuntu CA bundle and
installs root-owned copies at the Debian/Go path and bundled Python/OpenSSL path.
Finalization verifies both copies. A negative archive test rejects different CA
content. No TLS bypass or private/custom user trust store was introduced.

The intermediate image v4 passed import, but a fresh Python TLS probe revealed
that Python expected `/etc/ssl/cert.pem`; merely providing the Debian bundle path
was insufficient without hashed certificate directories. That failed result is
retained in `phase7-real-client-20260922.ca-tls.txt`. Image v5 adds and checks both
paths, and requires no manual package installation after import.

Final image identity: `1148d5f8-8d1f-4030-964d-66ad71adebd9`.
Image SHA-256: `29c60c7b08536ff5ae80eb80f1d2eaa7948c39dffcebb20763eb9ca3e1a9e2fe`.
Public CA SHA-256: `9481fcd95f41b221f02f14d896535fe500bec539bc563c4cdca1acee483a8bdd`.

- [Linux bootstrap tests/Clippy/build](phase7-real-client-20260922.ca-build-v5.txt): PASS.
- [Windows bootstrap tests](phase7-real-client-20260922.ca-windows-tests.txt): seven PASS.
- [Fresh v5 import/finalization](phase7-real-client-20260922.ca-import-v5.txt): PASS.
- [Fresh unprivileged TLS](phase7-real-client-20260922.ca-tls-v5.txt): 121 public roots,
  verified TLS 1.3 to api.openai.com, no credentials used.
- [Fresh v5 Host execution](phase7-real-client-20260922.ca-host-v5.txt): PASS, including
  import, persistent execution, save/reopen and MCP recovery preserving the worker.
- The client-preparation example passes native Clippy and actual Host execution.
- [Source snapshot](phase7-real-client-20260922.sources.json): 180 files. The earlier
  [full gate](phase7-managed-image-20260922.gate.txt) covers its earlier snapshot;
  it is not relabeled as a full gate for subsequent CA/example changes.

The v3 connection environment stays live to preserve the actual-client Session;
the corrected v5 image separately proves reproducible fresh provisioning. Retained
failed/intermediate validation distros are not release installations. Desktop,
actual-client execution, installer/update/repair and JAX remain open.
