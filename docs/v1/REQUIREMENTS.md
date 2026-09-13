# CoKernel v1 Requirements

Status: **implementation baseline**  
Scope: single Windows workstation / managed WSL compute runtime

## 1. Background

The user wants to use the CPU, memory, storage, and NVIDIA GPU already present in a Windows PC as a simple Linux-based Python/AI compute environment.

The desired workflow is centered on projects and notebook documents:

```text
Install CoKernel
  -> Create/Open Project
  -> Add packages
  -> Create/Import/Open .ipynb
  -> Run cells interactively
  -> Run multiple notebooks concurrently when needed
  -> Inspect resource use and failures
  -> Allow an AI to join the same execution state
```

The user should not need to manually construct or routinely operate WSL, Linux service configuration, Python virtual environments, process supervision, MCP transport, or tunnel infrastructure.

## 2. Product objective

**OBJ-001 (P0)**  
Turn a Windows NVIDIA GPU PC into a managed Linux notebook compute environment that is usable as a normal Windows application.

**OBJ-002 (P0)**  
Make Project and `.ipynb` the primary user-facing working concepts.

**OBJ-003 (P0)**  
Allow human and AI clients to operate on the same live execution state.

**OBJ-004 (P0)**  
Support multiple independent live executions concurrently on the same machine.

**OBJ-005 (P0)**  
Integrate MCP and secure tunneled remote AI access into the product rather than requiring separate manual infrastructure.

## 3. Priority definition

- **P0** — required for CoKernel v1 to satisfy its core purpose.
- **P1** — required for a practical v1 release but may follow the first end-to-end vertical slice.
- **P2** — desirable v1 enhancement; may be deferred without invalidating the core architecture.

## 4. Installation and runtime requirements

**R-INST-001 (P0)**  
The product shall install on supported Windows systems through a Windows-facing setup flow.

**R-INST-002 (P0)**  
The product shall create and manage a dedicated Linux environment through WSL without requiring routine manual WSL administration by the user.

**R-INST-003 (P0)**  
The managed Linux environment shall be able to use the supported NVIDIA GPU through the Windows/WSL GPU path.

**R-INST-004 (P0)**  
The product shall verify that the installed runtime can execute a representative GPU operation before reporting installation healthy.

**R-INST-005 (P1)**  
The product shall restore its intended running/stopped state after Windows sign-in/restart without requiring the user to manually start WSL components.

**R-INST-006 (P1)**  
The product shall provide repair and diagnostic operations for runtime failures.

**R-INST-007 (P0)**  
A detected legacy experimental CoKernel runtime may be explicitly destroyed and replaced by a fresh v1 runtime. Destruction shall not be silent in released builds.

## 5. Project requirements

**R-PROJ-001 (P0)**  
The system shall expose Project as the primary unit for Python source, data, notebooks, dependency definition, and execution environment.

**R-PROJ-002 (P0)**  
The system shall allow a user to create a new Project.

**R-PROJ-003 (P0)**  
The system shall allow a user to open an existing compatible Project.

**R-PROJ-004 (P0)**  
Each Project shall have a logically independent Python environment.

**R-PROJ-005 (P0)**  
Project dependency truth shall be represented by `pyproject.toml` and `uv.lock` and managed through `uv`.

**R-PROJ-006 (P0)**  
The system shall automatically create or synchronize the Project environment when necessary.

**R-PROJ-007 (P1)**  
Project storage and environment/cache storage shall be observable separately enough for the user to understand major disk usage.

## 6. Package/environment requirements

**R-ENV-001 (P0)**  
The system shall support adding a Python package to a Project.

**R-ENV-002 (P0)**  
The system shall support removing a Python package from a Project.

**R-ENV-003 (P0)**  
The system shall support synchronizing the Project environment to the locked dependency state.

**R-ENV-004 (P0)**  
The implementation shall use uv's cache/link behavior so logical Project isolation does not require unnecessary full physical duplication of identical packages.

**R-ENV-005 (P0)**  
Package/environment operations shall not silently destroy live Execution Session memory state.

**R-ENV-006 (P0)**  
When an environment changes while one or more Sessions for that Project are alive, the product shall make it clear that those Sessions are using the environment state from their process start and may require restart to use the new package state.

**R-ENV-007 (P1)**  
Package operation failures shall be surfaced in user-oriented terms with the relevant resolver/install evidence available for diagnostics.

## 7. Notebook document requirements

**R-NB-001 (P0)**  
The system shall treat standard `.ipynb` as a first-class notebook document format.

**R-NB-002 (P0)**  
The system shall create new `.ipynb` documents inside a Project.

**R-NB-003 (P0)**  
The system shall open existing `.ipynb` documents in a Project.

**R-NB-004 (P0)**  
The system shall import/upload an existing `.ipynb` from Windows into a Project without requiring the Windows filesystem to be permanently mounted into the managed workload environment.

**R-NB-005 (P0)**  
Import shall preserve supported code cells, Markdown cells, outputs, attachments, execution counts, and unknown metadata unless an explicit transformation is required by the notebook format version.

**R-NB-006 (P0)**  
The Project determines the execution environment used by a notebook. Notebook metadata shall not silently redirect execution to an unrelated environment.

**R-NB-007 (P0)**  
Notebook edits and execution outputs shall be persisted back to a valid `.ipynb` document.

**R-NB-008 (P0)**  
Notebook file state and live Execution Session memory state shall be presented as distinct concepts.

**R-NB-009 (P1)**  
The system shall detect external modification of an open notebook and avoid silently overwriting conflicting changes.

## 8. Interactive execution requirements

**R-EXEC-001 (P0)**  
A notebook shall be executable using the Python environment belonging to its Project without requiring the user to select a kernel identifier or executable path.

**R-EXEC-002 (P0)**  
Notebook execution shall support persistent state across cells, including variables, imports, and loaded objects.

**R-EXEC-003 (P0)**  
Python notebook execution shall provide interactive semantics appropriate for notebook use, including expression results, stdout/stderr, exceptions, rich MIME output, top-level asynchronous execution where supported, and IPython behavior used by typical Python notebooks.

**R-EXEC-004 (P0)**  
The product shall represent live execution state as an Execution Session.

**R-EXEC-005 (P0)**  
By default, one open/running notebook has at most one active primary Execution Session at a time.

**R-EXEC-006 (P0)**  
Execution requests within the same Session shall be serialized in a deterministic order.

**R-EXEC-007 (P0)**  
Different Sessions shall be capable of executing concurrently.

**R-EXEC-008 (P0)**  
Different notebooks in the same Project may have separate concurrent Sessions while sharing the same Project dependency environment.

**R-EXEC-009 (P0)**  
Different Projects may execute concurrently with independent environments.

**R-EXEC-010 (P0)**  
The user shall be able to interrupt the currently executing request without necessarily destroying the Session.

**R-EXEC-011 (P0)**  
The user shall be able to restart a Session, explicitly accepting loss of its in-memory state.

**R-EXEC-012 (P0)**  
The user shall be able to stop/terminate a Session.

## 9. Supervision and failure requirements

**R-SUP-001 (P0)**  
Notebook user code shall execute in a child process isolated from the CoKernel supervisory/control process so a Python/native-extension crash does not directly terminate the control plane.

**R-SUP-002 (P0)**  
CoKernel shall supervise each Session process and retain process exit evidence.

**R-SUP-003 (P0)**  
When a Session terminates unexpectedly, the system shall surface available evidence such as exit status/signal, recent stderr, last executing cell/operation, memory state, GPU state, and OOM evidence where obtainable.

**R-SUP-004 (P0)**  
The system shall distinguish Session failure from broader runtime/WSL/control-plane failure.

**R-SUP-005 (P0)**  
Unexpected Session death shall not silently discard the failure evidence by immediately replacing the Session. Restart shall be a visible state transition.

**R-SUP-006 (P1)**  
The system shall provide bounded logs and diagnostic export sufficient to investigate failed executions without exposing stored secrets.

## 10. Resource and parallelism requirements

**R-RES-001 (P0)**  
The product shall expose machine CPU utilization, host memory usage, WSL memory usage, GPU utilization, VRAM usage, and relevant storage usage.

**R-RES-002 (P0)**  
The product shall expose active Session count and per-Session state.

**R-RES-003 (P1)**  
Where technically reliable, resource usage shall be attributed to individual Sessions.

**R-RES-004 (P0)**  
The architecture shall not assume unlimited CPU/RAM/GPU/VRAM merely because Sessions can run concurrently.

**R-RES-005 (P1)**  
The product shall warn or fail clearly when a new execution cannot reasonably proceed because required local resources are unavailable.

## 11. Human/AI shared-state requirements

**R-AI-001 (P0)**  
An AI client shall be able to identify Projects, notebooks, and live Sessions through CoKernel's domain model.

**R-AI-002 (P0)**  
An AI client joining an existing notebook shall be able to operate on the same live Execution Session used by the human UI.

**R-AI-003 (P0)**  
AI attachment shall not silently create a second execution state when a primary live Session already exists for the target notebook.

**R-AI-004 (P0)**  
Human and AI execution requests targeting the same Session shall share one serialization/queue mechanism so execution order is defined.

**R-AI-005 (P0)**  
AI read-only intents shall have narrower operations than arbitrary code execution where feasible.

## 12. MCP requirements

**R-MCP-001 (P0)**  
CoKernel shall expose an MCP server for AI access.

**R-MCP-002 (P0)**  
MCP operations shall be expressed primarily in CoKernel domain concepts: Project, Notebook, Session, Environment, File, Variable, Execution, Metrics.

**R-MCP-003 (P0)**  
MCP tool schemas and annotations shall accurately describe their capability and side effects.

**R-MCP-004 (P0)**  
The MCP surface shall include read-only Project/notebook/session inspection, safe variable inspection, Session lifecycle, and notebook-cell execution operations needed for the core human/AI workflow.

**R-MCP-005 (P0)**  
Safe variable inspection shall not accept arbitrary Python expressions and shall not invoke uncontrolled user object representation behavior for unsupported values.

**R-MCP-006 (P0)**  
Operations that can execute notebook/user code shall remain honestly classified as potentially side-effecting/open-world capabilities.

## 13. Secure remote access requirements

**R-TUN-001 (P0)**  
A user shall be able to enable secure remote MCP access without configuring a public inbound port, router forwarding, a fixed public IP, or a public TLS endpoint.

**R-TUN-002 (P0)**  
Tunnel configuration and lifecycle shall be managed as part of CoKernel.

**R-TUN-003 (P0)**  
Remote access failure shall not stop local Project/Notebook execution.

**R-TUN-004 (P0)**  
Tunnel credentials and local MCP bearer credentials shall not be exposed to notebook user code.

**R-TUN-005 (P1)**  
Tunnel reconnect after runtime/control-plane restart shall be automatic when remote access remains enabled and credentials are valid.

## 14. Windows application requirements

**R-UI-001 (P0)**  
The user shall be able to perform the core workflow from a Windows application without routine command-line administration.

**R-UI-002 (P0)**  
The application shall expose Project, Packages, Notebooks, Sessions, Resources, MCP/Remote Access, Logs/Diagnostics, and product status in understandable terms.

**R-UI-003 (P0)**  
The application shall not require normal users to manipulate internal process/session identifiers to run a notebook.

**R-UI-004 (P0)**  
Usability and clarity take priority over visual ornamentation.

**R-UI-005 (P1)**  
Closing/reopening the visible UI shall not by itself destroy running Sessions when the background runtime is intended to remain running.

## 15. Security/trust requirements

**R-SEC-001 (P0)**  
The managed compute environment shall not automatically mount Windows user drives into notebook execution.

**R-SEC-002 (P0)**  
Windows executable interop shall remain disabled in the managed workload environment unless a future explicitly-scoped feature requires it.

**R-SEC-003 (P0)**  
Notebook user code shall not receive Windows credential stores, tunnel credentials, MCP bearer secrets, SSH keys, browser profiles, Docker sockets, or unrelated host credentials.

**R-SEC-004 (P0)**  
CoKernel shall clearly treat notebook code as user-controlled compute code rather than claiming malware-grade sandbox isolation.

**R-SEC-005 (P0)**  
Externally reachable control surfaces shall be authenticated and the product shall create no unnecessary public inbound listener.

## 16. Update and maintenance requirements

**R-UPD-001 (P1)**  
A v1 installation shall be updateable without recreating the entire WSL environment for routine v1-to-v1 updates.

**R-UPD-002 (P0)**  
Maintenance that would terminate active Sessions shall require explicit user acceptance or deferral.

**R-UPD-003 (P1)**  
Repair shall preserve Project source/notebook data unless the user explicitly chooses a destructive recovery operation.

**R-UPD-004 (P1)**  
Installed product update shall use versioned product artifacts rather than a development Git worktree as its normal mechanism.

## 17. Compatibility and quality requirements

**R-QUAL-001 (P0)**  
`.ipynb` documents saved by CoKernel shall remain structurally valid and usable by other compatible notebook tooling for the features CoKernel writes.

**R-QUAL-002 (P0)**  
The architecture shall preserve deterministic separation between document state, Project environment state, and live Session memory state.

**R-QUAL-003 (P0)**  
A failure in one Session shall not unnecessarily terminate unrelated Sessions.

**R-QUAL-004 (P1)**  
Core state transitions and external/local protocols shall be versioned and testable.

**R-QUAL-005 (P1)**  
Errors shown to users shall state the affected domain object, what is known, and an actionable next step where one exists.

## 18. Post-v1 extension requirements

The v1 architecture shall leave room for, but does not implement as release criteria:

- second-PC/headless CoKernel Node operation;
- RTX 5090 + RTX 3080 multi-machine resource pools;
- remote workspace/session routing;
- scheduling by CPU/RAM/GPU/VRAM requirements;
- CoKernel Fabric.

These extensions must be possible without redefining Project, Notebook, Environment, and Execution Session semantics.