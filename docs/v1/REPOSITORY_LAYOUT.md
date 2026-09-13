# CoKernel v1 Repository Layout

Status: **implementation baseline**

The `cokernel-v1` branch will progressively replace the experimental root layout with the following structure.

```text
CoKernel/
├─ Cargo.toml
├─ Cargo.lock
├─ rust-toolchain.toml
│
├─ crates/
│  ├─ cokernel-domain/
│  ├─ cokernel-protocol/
│  ├─ cokernel-runtime/
│  ├─ cokernel-host/
│  ├─ cokernel-mcp/
│  └─ cokernel-bootstrap/
│
├─ worker/
│  ├─ pyproject.toml
│  ├─ uv.lock
│  ├─ src/cokernel_worker/
│  └─ tests/
│
├─ desktop/
│  ├─ package.json
│  ├─ pnpm-lock.yaml
│  ├─ src/
│  └─ src-tauri/
│
├─ installer/
│  └─ windows/
│
├─ protocol/
│  ├─ fixtures/
│  └─ schemas/
│
├─ tests/
│  ├─ fixtures/notebooks/
│  ├─ integration/
│  ├─ e2e/
│  └─ security/
│
├─ tools/
│  └─ dev/
│
├─ legacy/                 # temporary during replacement only
│  └─ v0_1-reference/
│
├─ docs/
│  └─ v1/
│
└─ .github/workflows/
```

## 1. `cokernel-domain`

Pure/shared domain crate.

Contains:

- domain IDs/newtypes;
- Runtime/Project/Environment/Notebook/Session/Operation/Failure models;
- state enums/transitions that do not require OS access;
- error model;
- resource snapshot types.

Rules:

- no Windows/WSL process spawning;
- no uv subprocess implementation;
- no HTTP/MCP dependencies;
- highly unit-testable.

## 2. `cokernel-protocol`

Shared wire/protocol crate.

Contains:

- request/response/event envelopes;
- framed JSON codec;
- Host<->Runtime messages;
- Runtime local API messages;
- Worker protocol messages mirrored/translated as needed;
- protocol/capability version types;
- message size limits/defaults;
- compatibility tests/fixtures.

This crate depends on `cokernel-domain` but remains independent of Desktop/MCP.

## 3. `cokernel-runtime`

Linux/WSL runtime binary/library.

Modules:

```text
bridge/
project/
uv/
notebook/
session/
worker/
metrics/
diagnostics/
services/
state/
```

Subcommands for development/recovery may include:

```text
cokernel-runtime bridge --stdio
cokernel-runtime status --json
cokernel-runtime doctor --json
cokernel-runtime project ...
cokernel-runtime session ...
```

The stable product path is the bridge/domain API; CLI remains valuable for development/recovery.

## 4. `cokernel-host`

Windows background Host.

Modules:

```text
ipc/
wsl_bridge/
desired_state/
secrets/
metrics/
startup/
update/
notifications/
```

Build target: Windows x64 initially.

## 5. `cokernel-mcp`

MCP HTTP service.

Modules:

```text
auth/
tools/
runtime_client/
bounds/
transport/
```

It talks to Runtime Unix API and contains no worker-spawn or notebook persistence implementation.

## 6. `cokernel-bootstrap`

Native/bootstrap support shared by installer/repair where appropriate.

Responsibilities may include:

- Windows prerequisite detection;
- WSL distro management wrappers;
- runtime manifest verification;
- legacy detection;
- installer state/resume metadata.

Some installer logic may remain in dedicated setup technology; this crate prevents important checks from becoming untestable ad-hoc scripts.

## 7. Python worker package

`worker/` is intentionally separate from the Rust workspace.

Runtime artifacts build a versioned wheel/package for controlled injection with `uv run --with` or the selected verified uv overlay mechanism.

Worker package contains only execution-side functionality:

```text
protocol client
IPython shell integration
output capture
safe variable inspection
interrupt/shutdown handling
```

It must not grow into a second control plane.

## 8. Desktop

Tauri v2 shell.

Frontend recommended structure:

```text
src/
├─ app/
├─ api/
├─ components/
├─ features/projects/
├─ features/packages/
├─ features/notebooks/
├─ features/sessions/
├─ features/resources/
├─ features/remote-access/
├─ features/diagnostics/
└─ state/
```

`src-tauri` contains only Desktop-shell/native integration required to connect to Host IPC, tray, file picker, and app lifecycle. Runtime/business rules remain in Host/Runtime.

## 9. Frontend package manager

Use `pnpm` for the Desktop frontend repository dependencies.

Reasons:

- efficient content-addressed shared store/link model;
- reproducible lockfile;
- good fit for a small Tauri web frontend.

This choice is independent from Python Project management, which uses uv.

## 10. Protocol schemas/fixtures

`protocol/` stores language-neutral examples/schemas used by Rust/Python/frontend tests.

At minimum fixtures cover:

- handshake;
- status snapshot;
- Session execute/output/final;
- structured error;
- notebook import transaction;
- resource snapshot.

Rust types remain authoritative implementation structs; language-neutral fixtures prevent accidental protocol drift.

## 11. Notebook fixtures

`tests/fixtures/notebooks/` contains committed small notebooks covering:

- code + Markdown;
- stream/error/result output;
- HTML/PNG/SVG;
- attachments;
- unknown metadata;
- imported environment metadata;
- malformed/unsupported cases.

Binary-like embedded fixtures stay intentionally small.

## 12. Legacy source handling

Do not make new crates depend on current root PowerShell/Compose/Jupyter source.

During early implementation, v0.1 files may remain in place or be moved under `legacy/v0_1-reference` after their replacement capability is proven.

Removal is done in dedicated cleanup PRs so review clearly distinguishes feature implementation from historical-code deletion.

## 13. Build commands

Target developer commands:

```text
cargo test --workspace
cargo clippy --workspace --all-targets --all-features
cargo fmt --check

cd worker && uv sync --dev && uv run pytest

cd desktop && pnpm install --frozen-lockfile && pnpm test && pnpm build
```

A root task runner/Makefile/justfile may wrap these, but canonical underlying commands remain standard ecosystem tools.

## 14. Versioning

Product release version is shared across artifacts through build metadata/manifest generation.

Independently versioned compatibility values:

```text
host-runtime protocol
worker protocol
runtime state schema
MCP capability schema/version metadata where needed
```

Do not infer wire compatibility solely from semver string equality.

## 15. CI jobs

Target jobs:

```text
rust-linux
rust-windows
python-worker
frontend
protocol-fixtures
notebook-compat
mcp-contract
security-static
installer-build
```

Self-hosted/manual gates cover WSL/GPU/tunnel behavior not available on hosted runners.