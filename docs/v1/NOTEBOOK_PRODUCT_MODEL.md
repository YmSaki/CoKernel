# CoKernel v1 Notebook-first Product Model

Status: **v1 design decision**  
Depends on: `CONCEPTUAL_DESIGN.md`, `COMPONENT_MODEL.md`

## 1. Decision

CoKernel v1 is **not a JupyterLab product**.

The first-class user objects are:

1. **Project** — source/data/notebooks plus one uv-managed Python environment;
2. **Notebook (`.ipynb`)** — document format for interactive code, output, markdown, and metadata;
3. **Execution Session** — the live Python process bound to a notebook and project environment;
4. **Package Environment** — `pyproject.toml` + `uv.lock` + reconstructible `.venv`;
5. **Compute Runtime** — the managed WSL/Linux/GPU substrate.

Jupyter protocols and selected Jupyter components may remain implementation dependencies, but users should not need to understand Jupyter Server, kernelspecs, kernel IDs, Extension Manager, Jupyter configuration, or control-plane Python environments during normal use.

The v1 UX target is:

```text
Create/Open Project
    ↓
CoKernel creates/syncs project .venv with uv
    ↓
Add/remove packages from CoKernel UI or terminal
    ↓
Open .ipynb inside the project
    ↓
Run cells
    ↓
CoKernel automatically starts/attaches the correct project Python session
```

## 2. Why this change is necessary

The v0.1 JupyterLab-first UX leaks implementation concepts into ordinary work:

- users must understand the difference between Jupyter's runtime Python and notebook Python;
- JupyterLab's PyPI/Extension Manager appears to be a notebook package manager but targets the product runtime;
- kernel selection is visible even though the correct environment should be implied by the project;
- messages such as `Kernel Restarting` report an internal mechanism rather than the actual failure in terms the user can act on;
- package/environment operations are fragmented between terminal commands, Jupyter UI, kernelspecs, and Docker runtime details.

CoKernel v1 treats these as product-design defects, not documentation problems.

## 3. Project model

A Project is a directory with a stable identity and one default Python environment.

Minimum layout:

```text
project/
├─ pyproject.toml
├─ uv.lock
├─ .venv/                 # reconstructible
├─ notebooks/
│  └─ experiment.ipynb
├─ src/                   # optional
├─ data/                  # optional
└─ .cokernel/
   └─ project.json        # CoKernel metadata only
```

`pyproject.toml` + `uv.lock` are the dependency source of truth.

`.cokernel/project.json` must not duplicate Python dependency state. It may contain project identity, display preferences, preferred device/runtime policy, recent notebooks, and CoKernel-specific metadata.

### Project creation

`New Project` performs:

1. create/select directory;
2. initialize a minimal uv project if absent;
3. create/sync `.venv`;
4. install/register the project execution kernel/session adapter as needed;
5. mark project ready.

The user should not manually create a kernelspec.

## 4. Package management model

Packages belong to the Project, not to Jupyter.

Primary operations:

```text
AddPackage(name)
RemovePackage(name)
SyncEnvironment()
ListPackages()
EnvironmentStatus()
```

Implementation remains uv-first:

```bash
uv add <package>
uv remove <package>
uv sync
```

The Desktop UI may expose these operations graphically, while an integrated terminal remains available for advanced use.

The product-owned Jupyter/control-plane environment must never be presented as a normal mutable package target.

## 5. Notebook model

`.ipynb` is a first-class user document format.

Opening an `.ipynb` inside a Project means:

1. parse the notebook document;
2. identify its owning Project;
3. ensure the Project environment is valid/synchronized;
4. find or create the notebook's Execution Session;
5. bind UI and MCP to that same Execution Session;
6. execute cells using the Project environment.

No kernel picker is shown in the normal path when exactly one project environment applies.

Advanced/developer mode may expose execution backend/session details.

## 6. Execution Session model

The user-facing term is **Execution Session** or simply **Session**.

Internally it may be implemented by a Jupyter kernel process and Jupyter messaging protocol in v1.

Default policy:

- one notebook has one active Execution Session at a time;
- that Session uses the owning Project's `.venv`;
- browser/Desktop UI and AI attach to the same Session;
- a Session is not silently replaced while it is alive;
- restart is explicit unless automatic recovery is clearly safe and visible.

The product must not require the user to know a kernel ID.

## 7. Jupyter's role in v1

Jupyter is demoted from **product UX** to **compatibility/execution technology**.

### Retained initially

CoKernel may retain:

- `ipykernel`;
- Jupyter kernel messaging;
- Jupyter Server APIs where they materially reduce v1 implementation risk;
- existing Jupyter MCP integration while CoKernel's own narrower MCP contract is developed;
- optional JupyterLab as an advanced/compatibility frontend.

### Not authoritative

JupyterLab is not the source of truth for:

- project identity;
- package management;
- execution-environment selection;
- runtime health;
- session failure explanation;
- CoKernel settings;
- update/repair UX.

### Optional compatibility UI

`Open in JupyterLab` may remain available under Advanced/Compatibility actions, but it is not the primary CoKernel workflow.

## 8. Notebook UI

CoKernel Desktop v1 should provide a native/product-owned notebook surface or a tightly integrated embedded notebook surface capable of:

- file/project tree;
- markdown cells;
- code cells;
- outputs (text, rich display, images where supported);
- run cell / run all;
- interrupt;
- restart session;
- save `.ipynb`;
- session state;
- environment/package actions;
- resource usage visibility;
- AI-visible same-session semantics.

Implementation may use WebView2 and existing editor/rendering libraries. The architectural requirement is ownership by CoKernel, not a specific rendering technology.

## 9. Failure model: replace `Kernel Restarting`

A kernel/session process can die because of, for example:

- process crash;
- native-extension crash;
- CUDA/runtime failure;
- OS/WSL/container termination;
- memory exhaustion / OOM kill;
- explicit process exit;
- runtime restart/update.

The v1 UI must not reduce these to a generic `Kernel Restarting` modal.

CoKernel records and correlates:

- session process/container state;
- exit code/signal when available;
- stderr/log tail;
- WSL/kernel OOM evidence when available;
- GPU health immediately after failure;
- last executing cell/operation identifier;
- whether the Host/runtime restarted the environment.

User-facing examples:

```text
Execution session stopped unexpectedly.
Likely cause: Linux OOM killer terminated Python (exit 137).
Memory before failure: 19.6 / 20.0 GB WSL.

[Restart session] [View diagnostics]
```

or

```text
Python process crashed while executing cell B07:12.
CUDA device is still available.

[Restart session] [Open logs]
```

If the cause cannot be identified, say so explicitly instead of presenting an internal Jupyter state transition as the explanation.

## 10. MCP model under the notebook-first design

MCP operates on CoKernel domain objects:

```text
Project
Notebook
Execution Session
File
Package Environment
```

The same-session invariant remains mandatory.

Preferred v1 capabilities include:

- list/open projects;
- list/read notebooks;
- connect to existing notebook Session;
- create notebook;
- inspect safe variables;
- execute cell / code only through explicitly high-risk capabilities;
- inspect environment/package state.

Jupyter-specific identifiers remain adapter/internal fields where possible.

## 11. Issue #21 interpretation

Issue #21 is now broader than "fix Jupyter default kernel".

Its v1 architectural resolution is:

- Project environment determines notebook execution automatically;
- `/workspace/.venv` remains uv-managed;
- `/opt/cokernel` remains product-owned;
- JupyterLab Extension Manager is not a notebook package manager and must not be presented as one;
- Japanese/localization support is packaged by CoKernel, not installed by the user into the product runtime;
- if JupyterLab remains available, it is a compatibility UI configured to respect the project environment.

## 12. Acceptance criteria

- [ ] User can create a Project without understanding Jupyter or kernelspecs.
- [ ] Project creation results in a usable uv-managed `.venv`.
- [ ] User can add/remove/sync Python packages at Project scope.
- [ ] Opening a Project `.ipynb` and running a cell automatically uses the Project environment.
- [ ] The normal UI does not require kernel selection or kernel IDs.
- [ ] `sys.executable` resolves to the Project `.venv`.
- [ ] Human and AI operate on the same Execution Session.
- [ ] Session crashes surface actionable cause/evidence where available instead of only `Kernel Restarting`.
- [ ] `.ipynb` remains standards-compatible and can be opened by external Jupyter tooling.
- [ ] JupyterLab, if retained, is an optional compatibility frontend rather than the primary CoKernel UX.
