# CoKernel v1 agent instructions

These instructions apply to the `cokernel-v1` implementation.

## Authoritative specification

Before implementing a v1 feature, read `docs/v1/README.md` and the linked documents relevant to the component.

Do not infer v1 architecture from the experimental v0.1 Jupyter/Compose source tree. `docs/v1/` wins when old code/docs conflict.

## Product intent

CoKernel turns a Windows NVIDIA GPU PC into a managed Linux compute environment centered on:

```text
Project (uv)
  -> standard .ipynb NotebookDocument
     -> supervised persistent ExecutionSession
```

Human UI and AI/MCP clients operate on the same Runtime/domain objects. Different Sessions may execute concurrently; one Session serializes its execution queue.

## Non-negotiable invariants

- Project is the Python dependency/environment boundary.
- `pyproject.toml` + `uv.lock` are Project dependency truth; use uv rather than inventing a second resolver/store.
- `.ipynb` document state and live Session memory state are distinct.
- By default one notebook has one primary live Session.
- Human and AI targeting the same notebook use that same primary Session.
- One Session executes one user-code operation at a time; different Sessions may run concurrently.
- Notebook user code runs in a supervised child Python process, not in the Runtime/Host process.
- Workers emit structured results; they do not directly persist `.ipynb` files.
- Notebook Document Service owns revisioned/atomic product-mediated writes.
- Package/environment mutation never silently kills a live Session; mark it environment-stale and require explicit restart for new environment state.
- Session crash evidence is captured before recovery; do not hide failures behind an unexplained restart loop.
- Windows drives are not automatically mounted into workload execution.
- Windows executable interop remains disabled in the managed workload environment.
- Workers must not receive tunnel credentials, MCP bearer secrets, Windows credential stores, Docker sockets, SSH keys, browser profiles, or unrelated host secrets.
- Remote tunnel/MCP failure must not stop local notebook execution.
- No unnecessary public inbound listener.
- v0.1 -> v1 is a fresh-install/reset boundary; routine v1 updates preserve Project/notebook state.

## Technology baseline

- core/control/runtime: Rust;
- Python execution worker: Python + IPython;
- Python Project management: uv;
- Desktop: Tauri v2 + small TypeScript frontend;
- Desktop -> Host: current-user Windows Named Pipe;
- Host -> WSL Runtime: long-lived framed stdio bridge via `wsl.exe`;
- Runtime -> Worker: private Unix domain socket;
- AI: CoKernel-native MCP mapped to Runtime domain service;
- remote AI: managed secure outbound tunnel.

A technical spike may alter a technology choice only with a spec update/rationale in the same PR.

## Dependency direction

Allowed:

```text
Desktop -> Host IPC -> Host -> WSL Runtime bridge -> Runtime domain services -> Worker
AI -> Tunnel -> MCP -> Runtime domain services
```

Do not create shortcuts such as:

- Desktop directly spawning workers or running uv as the authoritative path;
- MCP directly spawning workers;
- worker writing notebook files;
- a second AI-only execution runtime.

## Implementation sequence

Follow `docs/v1/IMPLEMENTATION_ORDER.md` and umbrella issue #26.

Current phase issues:

- #27 Phase 0 spikes/repository reset
- #28 domain/protocol + uv Project manager
- #29 supervised Execution Sessions
- #30 `.ipynb` document/import service
- #31 Windows Host/WSL bridge
- #32 MCP/Tunnel/secrets
- #33 Desktop
- #34 installer/update/repair
- #35 full acceptance

Avoid giant PRs that skip lower-layer gates.

## Coding rules

### Rust

- model domain IDs as explicit newtypes where useful;
- prefer typed state/enums over stringly-typed internal logic;
- async/concurrency must preserve per-Session FIFO semantics;
- all external/subprocess input is bounded/validated;
- process children are explicitly supervised and cleaned up;
- structured errors use documented `CK-*` families;
- no `unwrap`/panic on normal external/user failure paths in long-running services.

### Python worker

- keep worker small and execution-focused;
- do not turn worker into environment/document/control manager;
- safe variable inspection must use identifier-only direct namespace lookup and exact-type bounded serialization;
- tests include hostile `__repr__`, `__str__`, `__iter__`, `__getattr__`, operators, and built-in subclasses;
- never place protocol frames on stdout where user/native code can corrupt them; use private Unix socket.

### Frontend

- UI calls Host IPC/domain operations; do not duplicate runtime rules in TypeScript;
- distinguish Document state from Session state;
- destructive operations clearly say whether they lose files or volatile memory;
- usability/clarity over visual complexity.

## Security-sensitive changes

Changes to any of these require explicit tests and relevant spec update if behavior/boundary changes:

- WSL automount/interop policy;
- Linux identities/permissions;
- Host/Runtime/Worker IPC;
- Project path/symlink handling;
- secret storage/handoff;
- MCP authentication/tool annotations;
- Tunnel lifecycle/auth;
- worker privilege/environment;
- update/legacy reset/data deletion.

## Required testing discipline

Every meaningful change should include the lowest practical test layer from `docs/v1/TEST_ACCEPTANCE.md`.

Never mark a P0 requirement complete because only a happy-path implementation exists.

Particularly preserve these canonical proofs:

1. persistent state: `x=123` then `x+1` -> 124;
2. parallel Sessions overlap but have isolated namespaces;
3. one Session serializes Human/AI operations;
4. worker crash produces FailureRecord and does not kill unrelated Session;
5. Windows notebook import remains valid `.ipynb` and uses target Project environment;
6. Human executes `secret_from_master=123456789`, MCP reads the same value from the same primary Session;
7. tunnel disconnect leaves local Session execution healthy;
8. diagnostics contain no known product secrets.

## v0.1 source policy

Old PowerShell/Compose/Jupyter/MCP-extension code is reference material until replacements are proven.

Do not build new v1 dependencies on old Jupyter/Compose service topology simply because code already exists.

Port hard-won knowledge (WSL provisioning, lifetime, tunnel sequencing, security/diagnostic lessons) into the new component boundaries, then retire legacy implementation in dedicated cleanup changes.