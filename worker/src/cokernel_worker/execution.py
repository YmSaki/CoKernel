from __future__ import annotations

import base64
from dataclasses import dataclass, field
import io
import sys
import traceback
from typing import Any

from IPython.core.interactiveshell import ExecutionResult, InteractiveShell
from IPython.utils.capture import RichOutput, capture_output

DEFAULT_STREAM_CAPTURE_BYTES = 4 * 1024 * 1024


def _binary_mime_requires_base64(mime: str) -> bool:
    return (mime.startswith("image/") and mime != "image/svg+xml") or mime in {
        "application/octet-stream",
        "application/pdf",
    }


def _normalize_mime_data(data: dict[str, Any]) -> dict[str, Any]:
    """Normalize IPython binary MIME values into notebook/wire-safe JSON values.

    IPython formatters commonly return raw ``bytes`` for image/png, image/jpeg,
    and PDF representations. Standard notebook MIME bundles carry those binary
    values as base64 text. Normalize at the execution boundary so downstream
    output budgeting and framed JSON transport never see raw bytes.
    """

    normalized: dict[str, Any] = {}
    for mime, value in data.items():
        if (
            type(mime) is str
            and _binary_mime_requires_base64(mime)
            and type(value) is bytes
        ):
            normalized[mime] = base64.b64encode(value).decode("ascii")
        else:
            normalized[mime] = value
    return normalized


@dataclass(slots=True)
class MimeBundle:
    data: dict[str, Any]
    metadata: dict[str, Any] = field(default_factory=dict)
    transient: dict[str, Any] = field(default_factory=dict)

    @classmethod
    def from_rich_output(cls, output: RichOutput) -> "MimeBundle":
        return cls(
            data=_normalize_mime_data(dict(output.data)),
            metadata=dict(output.metadata or {}),
            transient=dict(getattr(output, "transient", None) or {}),
        )


@dataclass(slots=True)
class ExecutionError:
    name: str
    value: str
    traceback: list[str]


@dataclass(slots=True)
class ExecutionOutcome:
    success: bool
    execution_count: int | None
    stdout: str
    stderr: str
    displays: list[MimeBundle]
    final_result: MimeBundle | None
    error: ExecutionError | None
    stdout_truncated_bytes: int = 0
    stderr_truncated_bytes: int = 0


class _BoundedTextCapture(io.StringIO):
    """StringIO-compatible capture that discards bytes after a hard UTF-8 budget."""

    def __init__(self, max_bytes: int) -> None:
        if type(max_bytes) is not int or max_bytes < 0:
            raise ValueError("max_bytes must be a non-negative integer")
        super().__init__()
        self.max_bytes = max_bytes
        self.stored_bytes = 0
        self.truncated_bytes = 0

    def write(self, value: str) -> int:
        if type(value) is not str:
            raise TypeError("write() argument must be str")
        encoded = value.encode("utf-8", errors="replace")
        remaining = self.max_bytes - self.stored_bytes
        stored_count = 0
        if remaining > 0:
            prefix = encoded[:remaining].decode("utf-8", errors="ignore")
            if prefix:
                stored = prefix.encode("utf-8")
                super().write(prefix)
                stored_count = len(stored)
                self.stored_bytes += stored_count
        self.truncated_bytes += len(encoded) - stored_count
        return len(value)


class ExecutionEngine:
    """Persistent IPython execution state for exactly one CoKernel Session.

    Production creates one engine inside one supervised worker process. The
    Session Supervisor, not this class, provides concurrency control and crash
    containment.
    """

    def __init__(
        self,
        shell: InteractiveShell | None = None,
        *,
        max_stream_capture_bytes: int = DEFAULT_STREAM_CAPTURE_BYTES,
    ) -> None:
        self.shell = shell or InteractiveShell.instance()
        self.shell.autoawait = True
        if type(max_stream_capture_bytes) is not int or max_stream_capture_bytes < 0:
            raise ValueError("max_stream_capture_bytes must be a non-negative integer")
        self.max_stream_capture_bytes = max_stream_capture_bytes

    def execute(self, source: str, *, cell_id: str | None = None) -> ExecutionOutcome:
        """Execute one cell using IPython semantics and capture notebook output."""

        stdout = _BoundedTextCapture(self.max_stream_capture_bytes)
        stderr = _BoundedTextCapture(self.max_stream_capture_bytes)
        with capture_output(stdout=False, stderr=False, display=True) as captured:
            previous_stdout = sys.stdout
            previous_stderr = sys.stderr
            sys.stdout = stdout
            sys.stderr = stderr
            try:
                result = self.shell.run_cell(
                    source,
                    store_history=True,
                    silent=False,
                    cell_id=cell_id,
                )
            finally:
                sys.stdout = previous_stdout
                sys.stderr = previous_stderr

        return self._outcome(
            result,
            stdout.getvalue(),
            stderr.getvalue(),
            captured.outputs,
            stdout_truncated_bytes=stdout.truncated_bytes,
            stderr_truncated_bytes=stderr.truncated_bytes,
        )

    def reset(self) -> None:
        """Discard user namespace/history while keeping the worker process alive."""

        self.shell.reset(new_session=True)

    def _outcome(
        self,
        result: ExecutionResult,
        stdout: str,
        stderr: str,
        outputs: list[RichOutput],
        *,
        stdout_truncated_bytes: int = 0,
        stderr_truncated_bytes: int = 0,
    ) -> ExecutionOutcome:
        error = result.error_before_exec or result.error_in_exec
        final_result: MimeBundle | None = None

        if error is None and result.result is not None:
            data, metadata = self.shell.display_formatter.format(result.result)
            final_result = MimeBundle(
                data=_normalize_mime_data(dict(data)),
                metadata=dict(metadata or {}),
            )

        execution_error: ExecutionError | None = None
        if error is not None:
            execution_error = ExecutionError(
                name=type(error).__name__,
                value=str(error),
                traceback=traceback.format_exception(type(error), error, error.__traceback__),
            )

        return ExecutionOutcome(
            success=error is None,
            execution_count=getattr(result, "execution_count", None),
            stdout=stdout,
            stderr=stderr,
            displays=[MimeBundle.from_rich_output(output) for output in outputs],
            final_result=final_result,
            error=execution_error,
            stdout_truncated_bytes=stdout_truncated_bytes,
            stderr_truncated_bytes=stderr_truncated_bytes,
        )
