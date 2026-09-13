from __future__ import annotations

from dataclasses import asdict
import json
import math
import os
import platform
import socket
import struct
import threading
import time
from typing import Any

import IPython

from .execution import ExecutionEngine
from .inspection import InspectionError, get_variable, list_variables
from .output_limits import OperationOutputBudget, OutputLimits, encode_json

PROTOCOL_V1 = 1
DEFAULT_MAX_FRAME_BYTES = 8 * 1024 * 1024
DEFAULT_HEARTBEAT_INTERVAL_SECONDS = 2.0
MAX_REQUEST_ID_CHARS = 256
MAX_METHOD_CHARS = 128
MAX_ERROR_SUMMARY_CHARS = 4096
INVALID_REQUEST_ID = "<invalid-request-id>"


class WorkerProtocolError(RuntimeError):
    pass


def _reject_non_json_constant(value: str) -> None:
    raise ValueError(f"non-standard JSON constant is not allowed: {value}")


def send_frame(
    sock: socket.socket,
    message: dict[str, Any],
    *,
    max_bytes: int = DEFAULT_MAX_FRAME_BYTES,
) -> None:
    try:
        payload = encode_json(message)
    except (TypeError, ValueError, OverflowError) as error:
        raise WorkerProtocolError("worker payload is not valid JSON") from error
    if len(payload) > max_bytes:
        raise WorkerProtocolError(f"frame exceeds maximum size: {len(payload)} bytes")
    sock.sendall(struct.pack(">I", len(payload)) + payload)


def receive_frame(
    sock: socket.socket, *, max_bytes: int = DEFAULT_MAX_FRAME_BYTES
) -> dict[str, Any] | None:
    prefix = _receive_exact(sock, 4)
    if prefix is None:
        return None
    size = struct.unpack(">I", prefix)[0]
    if size > max_bytes:
        raise WorkerProtocolError(f"frame exceeds maximum size: {size} bytes")
    payload = _receive_exact(sock, size)
    if payload is None:
        raise WorkerProtocolError("socket closed in the middle of a frame")
    try:
        value = json.loads(payload, parse_constant=_reject_non_json_constant)
    except (UnicodeDecodeError, ValueError) as error:
        raise WorkerProtocolError("frame is not valid UTF-8 JSON") from error
    if type(value) is not dict:
        raise WorkerProtocolError("frame root must be a JSON object")
    return value


def _receive_exact(sock: socket.socket, size: int) -> bytes | None:
    data = bytearray()
    while len(data) < size:
        chunk = sock.recv(size - len(data))
        if not chunk:
            if not data:
                return None
            raise WorkerProtocolError("socket closed in the middle of a frame")
        data.extend(chunk)
    return bytes(data)


class WorkerLoop:
    def __init__(
        self,
        session_id: str,
        engine: ExecutionEngine | None = None,
        *,
        output_limits: OutputLimits | None = None,
        heartbeat_interval_seconds: float = DEFAULT_HEARTBEAT_INTERVAL_SECONDS,
    ) -> None:
        self.session_id = session_id
        self.engine = engine or ExecutionEngine()
        self.output_limits = output_limits or OutputLimits()
        self.output_limits.validate(max_frame_bytes=DEFAULT_MAX_FRAME_BYTES)
        if type(heartbeat_interval_seconds) not in (int, float):
            raise ValueError("heartbeat_interval_seconds must be a positive finite number")
        try:
            heartbeat_interval = float(heartbeat_interval_seconds)
        except OverflowError as error:
            raise ValueError(
                "heartbeat_interval_seconds must be a positive finite number"
            ) from error
        if not math.isfinite(heartbeat_interval) or heartbeat_interval <= 0:
            raise ValueError("heartbeat_interval_seconds must be a positive finite number")
        self.heartbeat_interval_seconds = heartbeat_interval
        self._send_lock = threading.Lock()

    @property
    def heartbeat_interval_ms(self) -> int:
        return max(1, round(self.heartbeat_interval_seconds * 1000))

    def run(self, sock: socket.socket) -> None:
        self._send_event(
            sock,
            "ready",
            {
                "worker_version": _worker_version(),
                "python_version": platform.python_version(),
                "ipython_version": IPython.__version__,
                "pid": os.getpid(),
                "heartbeat_interval_ms": self.heartbeat_interval_ms,
            },
        )
        heartbeat_stop = threading.Event()
        heartbeat_thread = threading.Thread(
            target=self._heartbeat_loop,
            args=(sock, heartbeat_stop),
            name=f"cokernel-heartbeat-{self.session_id}",
            daemon=True,
        )
        heartbeat_thread.start()
        try:
            while True:
                request = receive_frame(sock)
                if request is None:
                    return
                if self._handle_request(sock, request):
                    return
        finally:
            heartbeat_stop.set()
            heartbeat_thread.join(timeout=1.0)

    def _heartbeat_loop(self, sock: socket.socket, stop: threading.Event) -> None:
        while not stop.wait(self.heartbeat_interval_seconds):
            try:
                self._send_event(
                    sock,
                    "heartbeat",
                    {
                        "monotonic_ns": time.monotonic_ns(),
                    },
                )
            except (OSError, WorkerProtocolError):
                return

    def _handle_request(self, sock: socket.socket, request: dict[str, Any]) -> bool:
        request_id = request.get("id")
        try:
            self._validate_request(request)
            method = request["method"]
            payload = request.get("payload", {})
            if type(payload) is not dict:
                raise WorkerProtocolError("request payload must be an object")

            if method == "ping":
                self._send_response(sock, request_id, {"pong": True})
                return False
            if method == "handshake":
                self._send_response(
                    sock,
                    request_id,
                    {
                        "protocol": PROTOCOL_V1,
                        "worker_version": _worker_version(),
                        "capabilities": [
                            "execute",
                            "inspect_variables",
                            "get_variable",
                            "reset",
                            "shutdown",
                        ],
                        "heartbeat_interval_ms": self.heartbeat_interval_ms,
                        "output_limits": {
                            "max_event_bytes": self.output_limits.max_event_bytes,
                            "max_operation_bytes": self.output_limits.max_operation_bytes,
                            "max_blob_bytes": self.output_limits.max_blob_bytes,
                        },
                    },
                )
                return False
            if method == "inspect_variables":
                variables = [
                    asdict(item) for item in list_variables(self.engine.shell.user_ns)
                ]
                self._send_response(sock, request_id, {"variables": variables})
                return False
            if method == "get_variable":
                name = payload.get("name")
                value = get_variable(self.engine.shell.user_ns, name)
                self._send_response(sock, request_id, asdict(value))
                return False
            if method == "reset":
                self.engine.reset()
                self._send_response(sock, request_id, {"reset": True})
                return False
            if method == "execute":
                self._execute(sock, request_id, payload)
                return False
            if method == "shutdown":
                self._send_response(sock, request_id, {"shutdown": True})
                return True
            raise WorkerProtocolError(f"unsupported worker method: {method}")
        except (WorkerProtocolError, InspectionError, KeyError, TypeError, ValueError) as error:
            self._send_error(sock, _response_request_id(request_id), error)
            return False

    def _execute(
        self, sock: socket.socket, request_id: Any, payload: dict[str, Any]
    ) -> None:
        operation_id = payload.get("operation_id")
        source = payload.get("source")
        cell_id = payload.get("cell_id")
        if type(operation_id) is not str or not operation_id:
            raise WorkerProtocolError("execute.operation_id must be a non-empty string")
        if operation_id != request_id:
            raise WorkerProtocolError("execute.operation_id must match request id")
        if type(source) is not str:
            raise WorkerProtocolError("execute.source must be a string")
        if cell_id is not None and type(cell_id) is not str:
            raise WorkerProtocolError("execute.cell_id must be a string when present")

        self._send_event(sock, "execution_started", {"operation_id": operation_id})
        outcome = self.engine.execute(source, cell_id=cell_id)
        sequence = 0
        budget = OperationOutputBudget(self.output_limits)

        def output(
            event: str,
            data: dict[str, Any],
            *,
            source_omitted_bytes: int = 0,
            source_reason: str | None = None,
        ) -> None:
            nonlocal sequence
            next_sequence = sequence + 1
            message = budget.prepare_event(
                event,
                {
                    "operation_id": operation_id,
                    "sequence": next_sequence,
                    **data,
                },
                build_message=self._event_message,
                source_omitted_bytes=source_omitted_bytes,
                source_reason=source_reason,
            )
            if message is not None:
                self._send_message(sock, message)
                sequence = next_sequence

        if outcome.stdout or outcome.stdout_truncated_bytes:
            output(
                "stdout",
                {"text": outcome.stdout},
                source_omitted_bytes=outcome.stdout_truncated_bytes,
                source_reason="stream_capture_limit",
            )
        if outcome.stderr or outcome.stderr_truncated_bytes:
            output(
                "stderr",
                {"text": outcome.stderr},
                source_omitted_bytes=outcome.stderr_truncated_bytes,
                source_reason="stream_capture_limit",
            )
        for display in outcome.displays:
            output("display_data", asdict(display))
        if outcome.final_result is not None:
            output(
                "execute_result",
                {
                    "execution_count": outcome.execution_count,
                    **asdict(outcome.final_result),
                },
            )
        if outcome.error is not None:
            output(
                "error",
                {
                    "ename": outcome.error.name,
                    "evalue": outcome.error.value,
                    "traceback": outcome.error.traceback,
                },
            )

        status = "SUCCEEDED" if outcome.success else "FAILED"
        self._send_event(
            sock,
            "execution_finished",
            {
                "operation_id": operation_id,
                "status": status,
                "execution_count": outcome.execution_count,
                "output_count": sequence,
                "output_truncated": budget.truncated,
                "output_omitted_bytes": budget.omitted_bytes,
                "output_truncation_reasons": sorted(budget.reasons),
            },
        )
        self._send_response(
            sock,
            request_id,
            {
                "operation_id": operation_id,
                "status": status,
                "execution_count": outcome.execution_count,
                "output_truncated": budget.truncated,
                "output_omitted_bytes": budget.omitted_bytes,
            },
        )

    def _validate_request(self, request: dict[str, Any]) -> None:
        if request.get("protocol") != PROTOCOL_V1:
            raise WorkerProtocolError("unsupported worker protocol")
        if request.get("type") != "request":
            raise WorkerProtocolError("worker accepts request frames only")
        if request.get("session_id") != self.session_id:
            raise WorkerProtocolError("request session_id does not match worker session")
        request_id = request.get("id")
        if type(request_id) is not str or not request_id:
            raise WorkerProtocolError("request id must be a non-empty string")
        if len(request_id) > MAX_REQUEST_ID_CHARS:
            raise WorkerProtocolError("request id exceeds maximum length")
        method = request.get("method")
        if type(method) is not str or not method:
            raise WorkerProtocolError("request method must be a non-empty string")
        if len(method) > MAX_METHOD_CHARS:
            raise WorkerProtocolError("request method exceeds maximum length")

    def _send_message(self, sock: socket.socket, message: dict[str, Any]) -> None:
        with self._send_lock:
            send_frame(sock, message)

    def _send_response(
        self, sock: socket.socket, request_id: Any, result: dict[str, Any]
    ) -> None:
        self._send_message(
            sock,
            {
                "protocol": PROTOCOL_V1,
                "type": "response",
                "id": request_id,
                "session_id": self.session_id,
                "ok": True,
                "result": result,
            },
        )

    def _send_error(
        self, sock: socket.socket, request_id: Any, error: Exception
    ) -> None:
        self._send_message(
            sock,
            {
                "protocol": PROTOCOL_V1,
                "type": "response",
                "id": request_id,
                "session_id": self.session_id,
                "ok": False,
                "error": {
                    "code": "CK-WORKER-REQUEST",
                    "summary": _bounded_text(str(error), MAX_ERROR_SUMMARY_CHARS),
                    "error_type": type(error).__name__,
                },
            },
        )

    def _event_message(self, event: str, payload: dict[str, Any]) -> dict[str, Any]:
        return {
            "protocol": PROTOCOL_V1,
            "type": "event",
            "session_id": self.session_id,
            "event": event,
            "payload": payload,
        }

    def _send_event(
        self, sock: socket.socket, event: str, payload: dict[str, Any]
    ) -> None:
        self._send_message(sock, self._event_message(event, payload))


def _response_request_id(value: Any) -> str:
    if type(value) is str and value and len(value) <= MAX_REQUEST_ID_CHARS:
        return value
    return INVALID_REQUEST_ID


def _bounded_text(value: str, max_chars: int) -> str:
    if len(value) <= max_chars:
        return value
    return value[: max_chars - 1] + "…"


def connect_and_run(socket_path: str, session_id: str) -> None:
    if not hasattr(socket, "AF_UNIX"):
        raise WorkerProtocolError(
            "Unix domain sockets are required for CoKernel worker runtime"
        )
    sock = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
    try:
        sock.connect(socket_path)
        WorkerLoop(session_id).run(sock)
    finally:
        sock.close()


def _worker_version() -> str:
    from . import __version__

    return __version__
