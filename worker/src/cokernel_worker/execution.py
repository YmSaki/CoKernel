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
MAX_EXCEPTION_NAME_CHARS = 256


def _binary_mime_requires_base64(mime: str) -> bool:
    return (mime.startswith("image/") and mime != "image/svg+xml") or mime in {
        "application/octet-stream",
        "application/pdf",
    }


def _normalize_mime_data(data: dict[str, Any]) -> dict[str, Any]:
    """Normalize IPython MIME values into notebook/wire-safe JSON values.

    IPython formatters commonly return raw ``bytes`` for image/png, image/jpeg,
    and PDF representations. Custom formatters may also return ``bytearray`` or
    ``memoryview`` values, and SVG is textual even when supplied as UTF-8 bytes.
    Normalize these at the execution boundary so downstream output budgeting and
    framed JSON transport never see bytes-like MIME payloads that have a standard
    notebook representation.
    """

    normalized: dict[str, Any] = {}
    for mime, value in data.items():
        if type(mime) is str and isinstance(value, (bytes, bytearray, memoryview)):
            raw = bytes(value)
            if _binary_mime_requires_base64(mime):
                normalized[mime] = base64.b64encode(raw).decode("ascii")
                continue
            if mime == "image/svg+xml":
                normalized[mime] = raw.decode("utf-8")
                continue
        normalized[mime] = value
    return normalized


def _safe_exception_name(error: BaseException) -> str:
    """Return a bounded non-empty exception name without invoking user hooks."""

    error_type = type(error)
    try:
        name = type.__getattribute__(error_type, "__name__")
    except Exception:
        return "Exception"
    if type(name) is not str or not name:
        return "Exception"
    if len(name) <= MAX_EXCEPTION_NAME_CHARS:
        return name
    return name[: MAX_EXCEPTION_NAME_CHARS - 1] + "…"


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

    def normalized(self) -> "MimeBundle":
        return MimeBundle(
            data=_normalize_mime_data(dict(self.data)),
            metadata=dict(self.metadata),
            transient=dict(self.transient),
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
        self._exhausted = False

    def write(self, value: str) -> int:
        if type(value) is not str:
            raise TypeError("write() argument must be str")
        encoded = value.encode("utf-8", errors="replace")
        if self._exhausted:
            self.truncated_bytes += len(encoded)
            return len(value)

        remaining = self.max_bytes - self.stored_bytes
        stored_count = 0
        if remaining > 0:
            prefix = encoded[:remaining].decode("utf-8", errors="ignore")
            if prefix:
                stored = prefix.encode("utf-8")
                super().write(prefix)
                stored_count = len(stored)
                self.stored_bytes += stored_count
        dropped = len(encoded) - stored_count
        self.truncated_bytes += dropped
        if dropped:
            # If the byte limit cuts through a multibyte character, there may
            # still be nominal byte capacity left even though the logical
            # stream prefix has already been truncated. Never resume capture
            # on a later write, or output after the omitted character could
            # appear ahead of the truncation boundary.
            self._exhausted = True
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
        final_result: MimeBundle | None = None
        displayhook = self.shell.displayhook
        original_write_output_prompt = displayhook.write_output_prompt
        original_write_format_data = displayhook.write_format_data
        original_finish_displayhook = displayhook.finish_displayhook

        def capture_execute_result(
            format_dict: dict[str, Any], metadata: dict[str, Any] | None = None
        ) -> None:
            nonlocal final_result
            # Keep formatter output raw until _outcome(). MIME normalization can
            # legitimately reject malformed custom formatter data (for example,
            # non-UTF-8 SVG bytes). Deferring normalization lets that failure be
            # converted into a structured failed ExecutionOutcome instead of
            # escaping after the worker already emitted execution_started.
            final_result = MimeBundle(
                data=dict(format_dict),
                metadata=dict(metadata or {}),
            )

        # ``run_cell`` installs ``shell.display_trap.hook`` as ``sys.displayhook``.
        # Patching the existing IPython DisplayHook preserves execution history,
        # ``_``/``Out`` namespace semantics, and ExecutionResult.result while
        # preventing terminal-style ``Out[n]:`` text from leaking into stdout.
        # Capturing the formatter payload here also avoids formatting the final
        # value a second time after ``run_cell`` returns.
        displayhook.write_output_prompt = lambda: None
        displayhook.write_format_data = capture_execute_result
        displayhook.finish_displayhook = lambda: setattr(
            displayhook, "_is_active", False
        )

        try:
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
        finally:
            displayhook.write_output_prompt = original_write_output_prompt
            displayhook.write_format_data = original_write_format_data
            displayhook.finish_displayhook = original_finish_displayhook

        return self._outcome(
            result,
            stdout.getvalue(),
            stderr.getvalue(),
            captured.outputs,
            final_result=final_result,
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
        final_result: MimeBundle | None = None,
        stdout_truncated_bytes: int = 0,
        stderr_truncated_bytes: int = 0,
    ) -> ExecutionOutcome:
        error = result.error_before_exec or result.error_in_exec

        execution_error: ExecutionError | None = None
        if error is not None:
            final_result = None
            execution_error = ExecutionError(
                name=_safe_exception_name(error),
                value=str(error),
                traceback=traceback.format_exception(type(error), error, error.__traceback__),
            )

        try:
            normalized_displays = [
                MimeBundle.from_rich_output(output) for output in outputs
            ]
            normalized_final_result = (
                final_result.normalized() if final_result is not None else None
            )
        except Exception as normalization_error:
            # Rich MIME values are user-controlled through formatter/display
            # hooks. Treat malformed notebook/wire representations as an
            # operation failure, but keep the supervised worker and persistent
            # namespace alive for subsequent cells.
            execution_error = ExecutionError(
                name=_safe_exception_name(normalization_error),
                value=str(normalization_error),
                traceback=traceback.format_exception(
                    type(normalization_error),
                    normalization_error,
                    normalization_error.__traceback__,
                ),
            )
            normalized_displays = []
            normalized_final_result = None

        return ExecutionOutcome(
            success=execution_error is None,
            execution_count=getattr(result, "execution_count", None),
            stdout=stdout,
            stderr=stderr,
            displays=normalized_displays,
            final_result=normalized_final_result,
            error=execution_error,
            stdout_truncated_bytes=stdout_truncated_bytes,
            stderr_truncated_bytes=stderr_truncated_bytes,
        )
