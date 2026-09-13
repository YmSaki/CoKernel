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
4. [`CONCEPTUAL_DESIGN.md`](CONCEPTUAL_DESIGN.md) — compact conceptual view.
5. [`ARCHITECTURE.md`](ARCHITECTURE.md) — target system architecture and technology choices.
6. [`COMPONENT_MODEL.md`](COMPONENT_MODEL.md) — component ownership and dependency boundaries.
7. [`EXECUTION_RUNTIME.md`](EXECUTION_RUNTIME.md) — supervised IPython worker/session semantics and parallel execution.
8. [`PROJECT_NOTEBOOK_MODEL.md`](PROJECT_NOTEBOOK_MODEL.md) — uv integration, notebook persistence/import, package changes.
9. [`INTERFACE_CONTRACTS.md`](INTERFACE_CONTRACTS.md) — Desktop/Host/WSL Runtime/Worker contracts.
10. [`MCP_TUNNEL.md`](MCP_TUNNEL.md) — domain-oriented MCP surface, permissions, tunnel integration.
11. [`UI_SPEC.md`](UI_SPEC.md) — Desktop screens, actions, states, failure UX.
12. [`SECURITY_OBSERVABILITY.md`](SECURITY_OBSERVABILITY.md) — trust boundaries, secrets, metrics, diagnostics, error model.
13. [`INSTALL_UPDATE.md`](INSTALL_UPDATE.md) — setup, legacy reset, auto-start, update/repair.
14. [`TEST_ACCEPTANCE.md`](TEST_ACCEPTANCE.md) — verification strategy and release acceptance gates.
15. [`REPOSITORY_LAYOUT.md`](REPOSITORY_LAYOUT.md) — target source/build repository structure.
16. [`IMPLEMENTATION_ORDER.md`](IMPLEMENTATION_ORDER.md) — implementation sequence and PR-sized milestones.
17. [`V0_1_ASSET_INVENTORY.md`](V0_1_ASSET_INVENTORY.md) — what knowledge/code from v0.1 is ported, referenced, or retired.
18. [`DECISIONS.md`](DECISIONS.md) — concise architectural decision record.

Language-neutral protocol examples live in [`../../protocol/`](../../protocol/README.md).

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

## Implementation choices fixed for v1

- core/control/runtime: Rust;
- Project/dependency/environment management: uv;
- notebook document: standard `.ipynb` / nbformat v4-compatible JSON;
- Python interactive semantics: supervised Project Python + IPython worker;
- parallelism: serialized within one Session, concurrent across Sessions;
- Desktop shell: Tauri v2 with a small TypeScript frontend;
- Desktop -> Host: Windows Named Pipe;
- Host -> WSL Runtime: long-lived framed stdio bridge;
- Runtime -> Worker: private Unix domain socket;
- AI: CoKernel-native MCP mapped to the same Runtime domain model;
- remote AI: managed outbound secure tunnel;
- v0.1 -> v1: explicit fresh-install/reset boundary.

## Definition of implementation-ready

The specification now fixes:

- domain ownership and lifecycle;
- Session concurrency/serialization semantics;
- Project/uv environment semantics;
- `.ipynb` persistence/import rules;
- Host/Runtime/Worker message contracts;
- MCP capability boundaries;
- secret/trust boundaries;
- installer/reset/update rules;
- Desktop UX boundaries;
- repository/build structure;
- acceptance tests and implementation order.

Implementation may proceed from Phase 0/1 of `IMPLEMENTATION_ORDER.md`. Any technical spike that changes a documented architectural choice must update this spec set before the dependent implementation is merged.