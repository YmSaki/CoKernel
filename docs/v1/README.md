# CoKernel v1 implementation specification

Status: **implementation baseline**  
Branch: `cokernel-v1`

This directory is the authoritative specification set for CoKernel v1. Implementation work should start from these documents rather than infer behavior from the experimental v0.1 source tree.

## Product statement

CoKernel turns a Windows PC into a managed Linux GPU compute environment centered on **Projects** and standard **`.ipynb` notebook documents**.

A Project owns its Python dependency definition and environment through `uv`. A Notebook is executed in a supervised, persistent **Execution Session** using the Project environment. Multiple Sessions can run concurrently. Human UI and AI/MCP clients can operate on the same Session. Remote AI access is provided through a managed secure tunnel.

The normal user experience is:

```text
Install CoKernel
  -> Create/Open Project
  -> Add packages
  -> Create/Import/Open .ipynb
  -> Run cells
  -> Inspect results/resources
  -> Optionally let AI join the same Session
```

WSL, process supervision, package caches, MCP transport, tunnel lifecycle, and execution plumbing are managed implementation details.

## Specification hierarchy

Read in this order:

1. [`REQUIREMENTS.md`](REQUIREMENTS.md) — v1 product/system requirements and priorities.
2. [`USE_CASES.md`](USE_CASES.md) — normal and exceptional user scenarios.
3. [`DOMAIN_MODEL.md`](DOMAIN_MODEL.md) — Project, Environment, Notebook, Session, Execution, Runtime.
4. [`ARCHITECTURE.md`](ARCHITECTURE.md) — target system architecture and technology choices.
5. [`COMPONENT_MODEL.md`](COMPONENT_MODEL.md) — component ownership and dependency boundaries.
6. [`EXECUTION_RUNTIME.md`](EXECUTION_RUNTIME.md) — supervised IPython worker/session semantics and parallel execution.
7. [`PROJECT_NOTEBOOK_MODEL.md`](PROJECT_NOTEBOOK_MODEL.md) — uv integration, notebook persistence/import, package changes.
8. [`INTERFACE_CONTRACTS.md`](INTERFACE_CONTRACTS.md) — Desktop/Host/WSL Runtime/Worker contracts.
9. [`MCP_TUNNEL.md`](MCP_TUNNEL.md) — domain-oriented MCP surface, permissions, tunnel integration.
10. [`SECURITY_OBSERVABILITY.md`](SECURITY_OBSERVABILITY.md) — trust boundaries, secrets, metrics, diagnostics, error model.
11. [`INSTALL_UPDATE.md`](INSTALL_UPDATE.md) — setup, legacy reset, auto-start, update/repair.
12. [`TEST_ACCEPTANCE.md`](TEST_ACCEPTANCE.md) — verification strategy and release acceptance gates.
13. [`IMPLEMENTATION_ORDER.md`](IMPLEMENTATION_ORDER.md) — implementation sequence and PR-sized milestones.
14. [`V0_1_ASSET_INVENTORY.md`](V0_1_ASSET_INVENTORY.md) — what knowledge/code from v0.1 is ported, replaced, or retired.
15. [`DECISIONS.md`](DECISIONS.md) — concise architectural decision record.

## Authority rules

- `REQUIREMENTS.md` defines **what must be achieved**.
- `ARCHITECTURE.md` and component/runtime documents define **how v1 will achieve it**.
- When older root documentation or v0.1 source behavior conflicts with this set, this set wins for `cokernel-v1`.
- A change that alters a requirement, domain invariant, public/local protocol, persistence rule, or security boundary must update the corresponding document in the same PR.
- Implementation details may evolve without specification changes when externally observable behavior and documented invariants remain unchanged.

## v1 scope boundary

Included in v1:

- one Windows machine with a managed WSL Linux compute environment;
- Project creation/opening and `uv`-managed Python environments;
- standard `.ipynb` create/import/edit/save/run flow;
- persistent supervised Python/IPython Execution Sessions;
- multiple concurrent Sessions;
- package add/remove/sync;
- CPU/RAM/GPU/VRAM/storage/status visibility;
- actionable Session crash evidence;
- local AI access through CoKernel MCP;
- secure tunneled remote MCP access;
- Windows Desktop/Tray control plane;
- first-install, legacy reset, update, repair, diagnostics.

Deferred until after v1:

- multi-PC CoKernel Fabric;
- remote RTX 3080/other Node scheduling;
- distributed execution;
- transparent live Session migration between machines;
- automatic multi-node resource scheduling.

## Definition of implementation-ready

Implementation may proceed when this specification set is internally consistent and the following are fixed:

- domain ownership and lifecycle;
- Session concurrency/serialization semantics;
- Project/uv environment semantics;
- `.ipynb` persistence/import rules;
- Host/Runtime/Worker message contracts;
- MCP capability boundaries;
- secret/trust boundaries;
- installer/reset/update rules;
- acceptance tests.

Those decisions are contained in this directory.