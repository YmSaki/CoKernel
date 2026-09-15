"""Bounded, synchronous output capture for the production worker.

There is no output queue here: the receiver applies wire budgets before a write
returns. A shared lock preserves cross-stream/display order, and closing the
capture seals saved stream/publisher references before terminal frames are sent.
"""
from __future__ import annotations

from contextlib import contextmanager
import io
import signal
import threading
from typing import Any, Callable, Iterator

from IPython.core.displaypub import DisplayPublisher
from IPython.core.interactiveshell import InteractiveShell

OutputSink = Callable[[str, dict[str, Any]], None]
MimeNormalizer = Callable[[dict[str, Any]], dict[str, Any]]
STREAM_CHUNK_CHARS = 8192


class StreamingText(io.TextIOBase):
    encoding = "utf-8"
    errors = "replace"

    def __init__(self, capture: StreamingCapture, name: str, max_bytes: int) -> None:
        super().__init__()
        self.capture = capture
        self.name = name
        self.max_bytes = max_bytes
        self.stored_bytes = 0
        self.truncated_bytes = 0
        self._exhausted = False

    def writable(self) -> bool:
        return True

    def getvalue(self) -> str:
        # The ExecutionOutcome must not replay already-streamed output.
        return ""

    def write(self, value: str) -> int:
        if type(value) is not str:
            raise TypeError("write() argument must be str")
        with self.capture.lock:
            if not self.capture.active:
                return len(value)
            # Do not encode/copy an arbitrarily large caller string all at once.
            for offset in range(0, len(value), STREAM_CHUNK_CHARS):
                chunk = value[offset : offset + STREAM_CHUNK_CHARS].encode(
                    "utf-8", errors="replace"
                )
                remaining = max(0, self.max_bytes - self.stored_bytes)
                prefix = (
                    chunk[:remaining].decode("utf-8", errors="ignore")
                    if not self._exhausted and remaining
                    else ""
                )
                kept = len(prefix.encode("utf-8"))
                self.stored_bytes += kept
                self.truncated_bytes += len(chunk) - kept
                if kept < len(chunk):
                    self._exhausted = True
                if prefix:
                    self.capture.sink(self.name, {"text": prefix})
        return len(value)

    def flush(self) -> None:
        # Every write is delivered synchronously; no trailing fragment exists.
        pass


class _StreamingPublisher(DisplayPublisher):
    def __init__(self, capture: StreamingCapture, **kwargs: Any) -> None:
        super().__init__(**kwargs)
        self.capture = capture

    def publish(
        self, data: Any, metadata: Any = None, source: Any = None,
        *, transient: Any = None, update: bool = False,
    ) -> None:
        if update:
            raise NotImplementedError("display updates require a worker-wire update event")
        self.capture.bundle("display_data", data, metadata, transient)

    def clear_output(self, wait: bool = False) -> None:
        # v1 has no clear event. Do not silently discard already-published data
        # or emit terminal ANSI escapes and pretend notebook clearing worked.
        raise NotImplementedError("clear_output requires a worker-wire clear event")


class StreamingCapture:
    def __init__(
        self, shell: InteractiveShell, sink: OutputSink, *,
        max_stream_bytes: int, normalize: MimeNormalizer,
    ) -> None:
        if type(max_stream_bytes) is not int or max_stream_bytes < 0:
            raise ValueError("max_stream_bytes must be a non-negative integer")
        self.shell = shell
        self.sink = sink
        self.normalize = normalize
        self.lock = threading.RLock()
        self.active = False
        self.normalization_error: Exception | None = None
        self.outputs: list[Any] = []  # Compatibility with buffered capture, always empty.
        self.stdout = StreamingText(self, "stdout", max_stream_bytes)
        self.stderr = StreamingText(self, "stderr", max_stream_bytes)
        self.publisher = _StreamingPublisher(self, shell=shell)
        self.previous_publisher: DisplayPublisher | None = None

    def __enter__(self) -> StreamingCapture:
        self.previous_publisher = self.shell.display_pub
        self.shell.display_pub = self.publisher
        self.active = True
        return self

    def __exit__(self, *args: Any) -> None:
        with self.lock:
            self.active = False
            self.shell.display_pub = self.previous_publisher

    def bundle(
        self, event: str, data: Any, metadata: Any = None,
        transient: Any = None, *, execution_count: int | None = None,
    ) -> None:
        with self.lock:
            if not self.active or self.normalization_error is not None:
                return
            try:
                payload: dict[str, Any] = {
                    "data": self.normalize(dict(data)),
                    "metadata": dict(metadata or {}),
                    "transient": dict(transient or {}),
                }
            except Exception as error:
                # Keep the first normalization failure, not a growing error list.
                self.normalization_error = error
                return
            if event == "execute_result":
                payload["execution_count"] = execution_count
            self.sink(event, payload)


class OutputInterruptGuard:
    """Defer SIGINT only across an output frame + sequence commit.

    Python invokes its signal handler on the main thread, including between
    sendall() and the sequence update. Raising there would produce a wire frame
    that the terminal output_count does not include. Deliver the original handler
    immediately after that short critical section, while still inside run_cell.
    """

    def __init__(self) -> None:
        self.installed = False
        self.depth = 0
        self.pending: tuple[int, Any] | None = None
        self.previous: Any = None

    def __enter__(self) -> OutputInterruptGuard:
        if threading.current_thread() is threading.main_thread():
            self.previous = signal.getsignal(signal.SIGINT)
            signal.signal(signal.SIGINT, self._handle)
            self.installed = True
        return self

    def __exit__(self, *args: Any) -> None:
        if self.installed:
            if signal.getsignal(signal.SIGINT) == self._handle:
                signal.signal(signal.SIGINT, self.previous)
            self.installed = False

    def _deliver(self, number: int, frame: Any) -> None:
        if callable(self.previous):
            self.previous(number, frame)
        elif self.previous != signal.SIG_IGN:
            signal.default_int_handler(number, frame)

    def _handle(self, number: int, frame: Any) -> None:
        if self.depth:
            self.pending = (number, frame)
        else:
            self._deliver(number, frame)

    @contextmanager
    def atomic(self) -> Iterator[None]:
        guarded = self.installed and threading.current_thread() is threading.main_thread()
        if guarded:
            self.depth += 1
        try:
            yield
        finally:
            if guarded:
                self.depth -= 1
                if self.depth == 0 and self.pending is not None:
                    pending, self.pending = self.pending, None
                    self._deliver(*pending)
