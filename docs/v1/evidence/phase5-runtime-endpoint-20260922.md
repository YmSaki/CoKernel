# Phase 5 shared Runtime endpoint evidence — 2026-09-22

Base: `f35ca99711a8d6c102ef9e47b857e0e377f87cbb` plus the working tree.
This is progress within **P5-07**, reopened after the user's architecture review.
It does not close P5-07, P5-GATE or the Desktop/MCP/JAX GPU delivery goal.
No commit, PR or issue closure was performed.

## Implemented boundary

RuntimeHandle submits all methods through one bounded request/ticket interface.
RuntimeOwner owns domain admission, background completions, correlation and
shutdown. The stdio bridge handles frames and tickets without managing Session
or package task collections. The public whole-service dispatch_async waiter was
removed. Duplicate caller request IDs receive distinct internal IDs and replies
are restored only to their original caller. Authenticated origin is separate
from JSON; authentication itself remains the transport's responsibility.

The endpoint bounds queued, in-flight and unread replies together at 32 and
checks the existing 8 MiB frame limit for direct clients. The bridge also bounds
its reply tasks. Dropping clients/tickets does not cancel admitted work. Owner
termination rejects unadmitted requests and joins cleanup. Rejected domain
shutdown preserves the endpoint for document/environment reconciliation.

This is a common client and lifetime boundary, not a claim that every domain
handler is free of storage waits. Document locking/filesystem operations, Project
registry persistence and some control queue waits still need the owned-operation
design tracked in DELIVERY_PLAN. Session-specific internal task routing remains.

## Component proofs

Seven new endpoint tests cover:

1. Duplicate client IDs, response correlation, shared primary after client/reply
   disconnect, and background completion without another caller driving it.
2. Capacity accounting including completed-but-unread replies and recovery when
   a reply is consumed.
3. Owner stop before admission: queued Project creation is rejected and creates
   no directory; closed handles reject further work.
4. Owner stop during actual launcher startup: pending caller receives failure and
   the owned launcher disappears.
5. Failed shutdown on externally modified dirty documents leaves the live edit
   readable. Explicit reload/reapply repair permits subsequent shutdown.
6. Oversized direct request rejection before domain mutation.
7. Human and MCP-origin clients editing the same revision use one authority:
   the first edit wins, the second gets CK-NB-CONFLICT, and the first edit saves.

Two Linux bridge-input tests cover fragmented read/EOF and dropping pending input
while a peer writer remains open, including executor termination.

## Same-Project notebook proof

The canonical real uv/IPython Session smoke now runs two different notebooks in
one Project. They load humanize 4.10.0 from the same file inside that Project's
.venv and return the same formatting result. Their base Python resolves to the
same binary, but they have different worker PIDs and distinct variable namespaces.
One notebook completes while the other's execution remains PENDING; interruption
then preserves the latter's marker. pyproject.toml and uv.lock remain byte-identical
through worker launches/executions. EOF reaps both workers and a held third launch.

An initial test compared sys.executable strings. Actual uv worker overlays used
different temporary executable paths, consistent with the existing overlay launch
design. The final proof checks actual Project package origin and dependency truth
instead of requiring equality of temporary overlay paths.

## Reproduced shutdown regression and correction

The first full gate reached the Project repair smoke, returned a successful
bridge.shutdown response, then failed to exit within ten seconds while the parent
kept stdin open. The new transport had started its next Tokio blocking stdin read
while awaiting a reply; dropping that future did not unblock executor shutdown.

Linux input now uses a duplicated close-on-exec descriptor, nonblocking readiness
and cancellable reads. Partial-frame futures remain intact during reply handling.
Unsupported non-readiness descriptors fail at startup. The existing ten-second
real process-exit check and the new pending-input tests pass without relaxing
timeouts. Real Windows Host/WSL input also passes with this implementation.

A later full-gate attempt reported ETXTBSY in the existing fake-worker restart
suite. Inspection found its temporary directory reservation used clock-derived
names and create_dir_all. The fixture now uses a UUID and exclusive create_dir
so it cannot reuse/overwrite another test's launcher directory. The precise cause
of the observed ETXTBSY was not independently reproduced; the focused 31-test
suite and final full gate pass, and the failure log is retained.

## Final verification

| Check | Result |
|---|---|
| Windows check.ps1 including actual WSL check.sh | PASS, exit 0 |
| Native Rust workspace | 76 unique tests PASS; fmt/check/clippy PASS |
| WSL Rust workspace | 183 unique tests PASS; fmt/check/clippy PASS |
| Native worker | 163 PASS, eight Unix-only skips |
| WSL worker | 171 PASS |
| Real worker/Session/notebook/package/Project/shared-environment smokes | PASS |
| Windows/WSL source identity before/after gate | 120 files match |
| Actual Windows Named Pipe/Host/WSL proof after input fix | PASS, exit 0 |
| Independent saved notebook and worker-exit postconditions | PASS, exit 0 |

Nested credential test subprocesses are counted once. Final documentation/evidence
edits follow source verification; no product assertion or acceptance timeout was
weakened. The Host proof uses unchanged production code relative to the final gate;
only the test fixture directory reservation changed afterward.

Final isolated Host root:
`/home/ymsaki/.cache/cokernel-v1-dev/host-proof-d94316a612d4`.
Independent verification checks valid saved nbformat, output 124, attachment,
shutdown metadata, absent failed import and worker PID exit. Host proof also checks
the shared marker 123456789 and responsive status during pending inspection.

Artifacts: [full gate](phase5-runtime-endpoint-20260922.gate.txt),
[Host and postconditions](phase5-runtime-endpoint-20260922.real.txt),
[source hashes](phase5-runtime-endpoint-20260922.sources.json),
[input shutdown failure](phase5-runtime-endpoint-20260922.input-before.txt),
[fixture busy failure](phase5-runtime-endpoint-20260922.fixture-before.txt).

These remain component/Rust clients on development Ubuntu. Actual Desktop,
Codex/web ChatGPT MCP, managed-distro security and JAX GPU computation are unproven.
Next is owned Document/Project storage work with preserved Notebook/Project order.
