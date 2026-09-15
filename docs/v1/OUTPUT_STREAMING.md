# CoKernel v1 live worker output contract

Status: implementation supplement to [EXECUTION_RUNTIME.md](EXECUTION_RUNTIME.md), sections 7-9.
Scope: Phase 3 production Python worker; not a declaration that the Runtime/WSL gate passes.

## Production path

`WorkerLoop._execute` calls the same persistent `ExecutionEngine.execute` with an
output sink. It does not create another interpreter, kernel, Session, or execution
service. No Jupyter Server/ipykernel dependency is added.

Python stdout/stderr writes, explicit rich display, and final-expression output
are delivered synchronously while the cell is executing. A write does not wait
for cell completion. Once handed to the socket, earlier output can be observed
even if a later user-code statement blocks or exits the process abruptly.

The engine's no-sink call remains a buffered convenience interface for direct
callers and component tests. The production worker always supplies the sink;
there is no fallback from live IPC to buffered execution.

## Ordering and lifecycle

- Existing v1 event names and envelopes are unchanged. No undocumented event is
  sent to the Rust decoder.
- A shared capture lock serializes stream writes and rich publishes. Sequential
  user-code output retains its actual cross-stream/display order; concurrent
  threads have lock-acquisition order, not an invented global source-code order.
- Streams are split into chunks of at most 8192 input characters before UTF-8
  encoding. Chunk boundaries are transport details, not notebook output classes.
  Consumers must not assume one event per stream or one event per `print()`.
- `sequence` is contiguous across output events actually sent. Heartbeats do not
  consume output sequence numbers. `execution_finished.output_count` counts sent
  output events, including an emitted terminal error.
- Final-expression output is formatted once through the existing IPython
  DisplayHook. Its `prompt_count` is used as execution count, matching terminal
  execution accounting while preserving `_` and `Out` history.
- The capture is sealed before terminal frames. Saved references to its stream
  or publisher cannot emit into a finished operation or a subsequent one. This
  is not isolation of arbitrary user-created background tasks using new global
  stdout references; background task ownership is a separate lifecycle concern.
- On the production main thread, SIGINT is deferred across output budgeting,
  framed write, and sequence commit, then delivered to the previous handler
  inside `run_cell`. An interrupt cannot leave a sent frame uncounted merely by
  landing between the write and sequence increment.

## Retention, limits, and failure handling

`StreamingCapture` retains no growing list of rich displays and no full stream
contents. Each rich bundle is normalized, budgeted, and sent before the next
publish. Shallow bundle construction avoids `dataclasses.asdict` recursively
calling user `__deepcopy__` hooks.

Existing per-stream source-byte caps and event/operation/blob wire budgets still
apply. Source bytes omitted after a stream cap are added to terminal
`output_omitted_bytes` with `stream_capture_limit`. A zero source budget does not
create an empty output event for every attempted write. Consumers must inspect
terminal accounting even when no output event remains to carry truncation data.

Once a stream is truncated, later writes do not resume beyond the omitted
prefix, including when the cap splits a multibyte UTF-8 character. Wire-level
stream truncation also closes that stream's output prefix for the operation.
Terminal event and response agree on truncation flags, omitted bytes, and unique
reason strings.

Malformed MIME normalization becomes `CoKernelOutputNormalizationError`; invalid
wire JSON becomes `CoKernelOutputTransportError` with a constant diagnostic that
does not stringify the invalid user payload. The operation finishes as FAILED
while the namespace remains available. Already-sent output is not retracted or
replayed. Subsequent user statements may still execute after an output failure;
this is not transaction rollback of Python side effects.

This closes accumulated output retention, not every possible memory allocation:
user-owned objects, formatter allocations, normalization of a single oversized
bundle, and temporary JSON encoding are not guaranteed to fit a process-wide
memory ceiling. The first normalization error may retain its traceback until
the operation ends. Process resource controls remain Supervisor responsibilities.

## Explicit remaining limitations

The current worker wire has no clear/update-display event. `clear_output()` and
`publish(update=True)` therefore raise `NotImplementedError` in the live path,
producing a normal failed operation rather than emitting terminal ANSI escapes,
silently ignoring the request, or appending an update as a new display.

Completing display mutation requires a versioned producer/consumer contract,
Rust event validation and sequence handling, Notebook output conversion, and
clear(wait)/display-id/update compatibility fixtures. Do not implement it only
in Python while the Runtime rejects the new frame shape.

This change does not add capture of arbitrary native file-descriptor writes or
subprocess output. It does not guarantee interrupt latency when a receiver stops
reading: synchronous writes apply backpressure, and Supervisor terminate/restart
escalation remains necessary for unresponsive execution.

## Verification entry points

`worker/tests/test_streaming_output.py` contains component regressions and three
actual `python -m cokernel_worker` AF_UNIX subprocess tests: pre-completion output,
SIGINT plus same-namespace reuse, and flushed output followed by abrupt exit 23.
`test_output_limits.py` and `test_protocol_contract.py` check the updated live
sink and chunked-stream accounting rather than the former buffered replay.

Run the complete canonical worker and Rust/WSL checks before declaring the Phase
3 integration gate passed. Passing only the selected source-snapshot tests does
not close #29 or #40.
