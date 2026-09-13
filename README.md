# CoKernel v1

**Projects, notebooks, local GPU compute, and AI sharing one execution state.**

> Branch status: v1 architecture/specification complete; implementation starts from `docs/v1/IMPLEMENTATION_ORDER.md`.

CoKernel v1 is a Windows application that turns an NVIDIA GPU PC into a managed Linux compute environment.

The primary model is deliberately small:

```text
Windows CoKernel
  -> managed WSL Linux runtime
     -> Project (uv)
        -> standard .ipynb Notebook
           -> supervised persistent Execution Session
```

- **Project** owns Python dependency/environment state through `uv`.
- **`.ipynb`** is the durable notebook document.
- **Execution Session** is the live Python/IPython process and namespace.
- Different Sessions can execute concurrently.
- Human UI and AI/MCP clients can operate on the same primary Session.
- CoKernel supervises Session failures and shows available exit/OOM/GPU/runtime evidence.
- Remote AI access is provided through CoKernel MCP + a managed secure outbound tunnel.

The normal user workflow is:

```text
Install CoKernel
 -> Create/Open Project
 -> Add packages
 -> Create/Import/Open .ipynb
 -> Run cells
 -> Inspect results/resources
 -> Optionally let AI join the same Session
```

WSL administration, process supervision, package cache details, MCP transport, and tunnel lifecycle are product-managed implementation details.

## v1 technology baseline

```text
Windows Desktop:   Tauri v2 + small TypeScript frontend
Windows Host:      Rust
WSL Runtime:       Rust
Project manager:   uv
Notebook document: standard .ipynb / nbformat v4-compatible JSON
Python execution:  supervised Project Python + IPython worker
Desktop -> Host:   Windows Named Pipe
Host -> WSL:       persistent framed stdio bridge through wsl.exe
Runtime -> Worker: private Unix domain socket
AI:                CoKernel-native MCP
Remote AI:         managed secure tunnel
```

## Specification

The authoritative implementation packet is [`docs/v1/README.md`](docs/v1/README.md).

Start with:

- [`docs/v1/REQUIREMENTS.md`](docs/v1/REQUIREMENTS.md)
- [`docs/v1/USE_CASES.md`](docs/v1/USE_CASES.md)
- [`docs/v1/DOMAIN_MODEL.md`](docs/v1/DOMAIN_MODEL.md)
- [`docs/v1/ARCHITECTURE.md`](docs/v1/ARCHITECTURE.md)
- [`docs/v1/IMPLEMENTATION_ORDER.md`](docs/v1/IMPLEMENTATION_ORDER.md)
- [`docs/v1/TEST_ACCEPTANCE.md`](docs/v1/TEST_ACCEPTANCE.md)

Implementation tracking is umbrella issue **#26** with phase issues **#27–#35**.

## Current source tree

The repository still contains experimental v0.1 PowerShell/Compose/Jupyter code as reference while replacement components are built.

For v1:

- do not infer architecture from the old service topology;
- do not add new dependencies on the old Jupyter/Compose runtime;
- port useful operational knowledge into the new component boundaries;
- retire old implementation only after the replacement capability passes its acceptance gate.

See [`docs/v1/V0_1_ASSET_INVENTORY.md`](docs/v1/V0_1_ASSET_INVENTORY.md).

## Implementation order

```text
Phase 0   feasibility spikes + repository reset
Phase 1   domain/protocol core
Phase 2   Project + uv environment manager
Phase 3   supervised IPython Sessions + parallelism
Phase 4   .ipynb document/import service
Phase 5   Windows Host + WSL bridge
Phase 6   CoKernel-native MCP
Phase 7   Secure Tunnel + secrets
Phase 8   Desktop UI
Phase 9   installer + legacy reset
Phase 10  update/repair/product hardening
Phase 11  full Windows/NVIDIA/MCP/tunnel acceptance
```

See [`docs/v1/IMPLEMENTATION_ORDER.md`](docs/v1/IMPLEMENTATION_ORDER.md) for gates and PR decomposition.

## Core correctness proofs

v1 is not considered complete until at least these are demonstrated:

1. Project package state is managed through uv and reconstructible.
2. A notebook cell can set `x = 123`; a later cell in the same Session evaluates `x + 1` to `124`.
3. Two notebook Sessions can execute concurrently with isolated namespaces.
4. Killing one worker produces failure evidence without killing another Session.
5. A Windows `.ipynb` can be imported into a Project and executed using that Project environment.
6. Human executes `secret_from_master = 123456789`; MCP reads `123456789` from that exact same primary Session.
7. Secure Tunnel can expose the MCP path remotely while tunnel failure leaves local compute running.
8. A fresh Windows install provisions the managed WSL/GPU runtime without manual Linux setup.

## Development instructions

Coding agents and contributors must read [`AGENTS.md`](AGENTS.md) before implementing v1 changes.