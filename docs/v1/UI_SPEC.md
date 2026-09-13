# CoKernel v1 Desktop UI Specification

Status: **implementation baseline**

## 1. UX principle

The UI exists to make Project/environment/notebook/session state understandable and operable. Visual ornamentation is secondary.

Normal users should not need to understand internal process IDs, runtime sockets, WSL commands, or protocol identifiers.

## 2. Navigation model

Primary navigation:

```text
Home
Projects
Sessions
Resources
Remote AI
Diagnostics
Settings
```

Opening a Project transitions into a Project workspace view with:

```text
Files/Notebooks
Packages
Notebook Editor
Project status
```

## 3. Home

Shows:

- overall Runtime state;
- GPU summary;
- CPU/RAM/WSL RAM summary;
- active Session count;
- Remote AI status;
- recent Projects;
- important warnings/errors;
- quick actions: New Project, Open Project, Diagnostics.

Example semantic layout:

```text
CoKernel          HEALTHY
RTX 5090          GPU 32%  VRAM 8.4/32 GB
CPU 14%           RAM 21/64 GB   WSL 9/20 GB
Sessions: 3       Remote AI: Connected

Recent Projects
- llm-tests
- image-lab
```

## 4. Projects list

Each card/row shows:

```text
Project name
path
Python version
Environment state
live Session count
last opened
```

Actions:

```text
Open
New Project
Register/Open existing Project
Remove from list
Delete Project files (separate destructive action)
```

## 5. Project workspace

Header shows:

```text
Project name
Environment: READY | SYNCING | BROKEN
Python version
Environment generation
live Sessions
```

Tabs/panes:

```text
Notebooks / Files
Packages
```

Environment changes produce a visible warning on stale Sessions, e.g.:

```text
Packages changed. 2 running Sessions still use their previous environment state.
[Restart sessions...] [Keep running]
```

No automatic memory-destroying action occurs from the warning alone.

## 6. Package UI

Features:

- search/input package spec;
- Add;
- Remove;
- Sync;
- installed/resolved version list;
- operation progress;
- resolver/install failure details.

The UI says what happened in Project terms; it may expose raw uv logs behind `Details`.

## 7. Notebook list/create/import

Actions:

```text
New Notebook
Import Notebook
Open
Rename/Move (when implemented safely)
Delete
```

Import launches Windows file picker and uses the controlled transfer API.

On name collision, UI requires explicit resolution; no silent overwrite.

## 8. Notebook editor

Minimum v1 editor capabilities:

- ordered cells;
- code cell editor;
- Markdown source/render toggle or practical rendering flow;
- add/delete/move cells;
- Run Cell;
- Run All/Run selected sequence if implemented;
- current execution indicator;
- outputs under cell;
- save/autosave status;
- external-change/conflict warning;
- Session status.

A simple usable cell editor is preferred over replicating a full IDE.

## 9. Notebook/session header

Shows separately:

```text
Document: Saved | Saving | Conflict | Error
Session: Not started | Starting | Idle | Running | Stale environment | Crashed | Stopped
```

This distinction is mandatory.

Actions:

```text
Start Session (when absent)
Interrupt current execution
Restart Session
Stop Session
```

Restart/Stop that loses memory asks for confirmation when live state exists.

## 10. Execution queue UI

When operations are queued in a shared Human/AI Session, UI can show:

```text
Running: cell abc (Human)
Queued:  cell def (AI)
```

The user does not need to manage the queue normally, but order/origin should be inspectable to explain why a cell has not started yet.

## 11. Session crash UI

Do not show only an internal restart label.

Example:

```text
Execution Session crashed

Known evidence:
- Python process exited with SIGKILL
- Linux OOM event detected near the same time
- WSL memory: 19.9 / 20.0 GB
- Last operation: cell 17

Probable classification: Out-of-memory

[Restart Session] [View evidence] [Leave stopped]
```

Unknown case:

```text
Execution Session crashed
Cause could not be determined from available evidence.
Exit status: ...
[View evidence]
```

## 12. Sessions page

Lists all live/recent Sessions:

```text
Project
Notebook
state
worker PID (Details only)
uptime
environment generation/stale state
queue/current operation
CPU/RAM
GPU/VRAM attribution if available
```

Actions: Open Notebook, Interrupt, Restart, Stop, View Failure.

PID is diagnostic detail, not normal selection input.

## 13. Resources page

Machine/runtime dashboard:

- CPU;
- host RAM;
- WSL RAM;
- storage;
- GPU utilization;
- VRAM;
- temperature/power when available;
- Session attribution where reliable.

The page may also show Project/cache storage breakdown when available.

## 14. Remote AI page

Shows:

```text
MCP: Healthy / Error
Remote Access: Disabled / Connecting / Connected / Degraded
last connection/error
```

Actions:

```text
Enable remote access
Set/replace credentials
Disable
Reconnect
View MCP capability summary
```

Local bearer/token values are not displayed by default.

## 15. Diagnostics page

Component tree:

```text
Windows Host
WSL Runtime
uv
GPU
MCP
Tunnel
```

Features:

- current health;
- latest errors;
- recent structured logs;
- run checks;
- Export Diagnostic Bundle;
- Repair.

## 16. Settings

Initial settings:

- start CoKernel automatically;
- desired runtime start behavior;
- UI language/theme as appropriate;
- update channel/policy;
- resource display cadence/preferences;
- Project default location;
- advanced/developer diagnostics toggle.

Settings must not expose implementation knobs simply because they exist internally.

## 17. Tray behavior

Tray states:

```text
Healthy
Degraded
Error
Stopped
Updating
```

Tray menu:

```text
Open CoKernel
Runtime status
Open recent Project
Start/Stop Runtime
Diagnostics
Exit UI
```

`Exit UI` does not implicitly stop Runtime/Sessions.

## 18. Notifications

Notify for events requiring attention:

- Session crashed;
- Runtime error;
- Remote access repeatedly failed;
- update requires decision;
- Project environment operation failed;
- disk/storage critically low.

Do not spam repetitive retry notifications; group/debounce them.

## 19. Destructive action language

Actions that lose durable files or volatile Session memory explicitly say what is lost.

Examples:

```text
Restart Session — loses in-memory Python variables/objects; notebook file remains.
Delete Project files — removes source/data/notebooks from Project storage.
Legacy Reset — unregisters the old WSL distro and permanently deletes its filesystem.
```

## 20. Accessibility/usability baseline

- keyboard-accessible primary actions;
- status not conveyed by color alone;
- selectable/copyable error/evidence text;
- progress/cancel for long operations;
- no modal dialog storms for normal background state changes;
- consistent terms from `DOMAIN_MODEL.md`.

## 21. Initial implementation scope

The first Desktop vertical slice needs only:

1. runtime status;
2. Project list/create;
3. package add/sync;
4. notebook create/import/open;
5. code-cell edit/run/output;
6. Session state/interrupt/restart/stop;
7. basic resources;
8. failure evidence.

Remote AI, diagnostics, update/settings can be layered after the local notebook vertical slice proves the model.