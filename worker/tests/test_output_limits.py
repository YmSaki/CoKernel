from __future__ import annotations

from dataclasses import dataclass
import json
import socket
import threading

import pytest

from cokernel_worker.execution import ExecutionOutcome, MimeBundle
from cokernel_worker.output_limits import (
    SERDE_JSON_MAX_INTEGER,
    SERDE_JSON_MIN_INTEGER,
    encode_json,
)
from cokernel_worker.protocol import (
    OutputLimits,
    PROTOCOL_V1,
    WorkerLoop,
    receive_frame,
    send_frame,
)


class FakeShell:
    user_ns = {}


@dataclass
class FakeEngine:
    outcome: ExecutionOutcome
    shell: FakeShell = FakeShell()

    def execute(self, source: str, *, cell_id: str | None = None) -> ExecutionOutcome:
        return self.outcome

    def reset(self) -> None:
        pass


def request(method: str, payload=None, request_id: str = "r1"):
    return {
        "protocol": PROTOCOL_V1,
        "type": "request",
        "id": request_id,
        "session_id": "s1",
        "method": method,
        "payload": payload or {},
    }


def run_execute(outcome: ExecutionOutcome, limits: OutputLimits):
    server, client = socket.socketpair()
    loop = WorkerLoop("s1", FakeEngine(outcome), output_limits=limits)
    thread = threading.Thread(target=loop.run, args=(server,), daemon=True)
    thread.start()
    ready = receive_frame(client)
    assert ready and ready["event"] == "ready"

    send_frame(
        client,
        request("execute", {"operation_id": "op1", "source": "ignored"}, "op1"),
    )
    frames = []
    while True:
        frame = receive_frame(client)
        assert frame is not None
        frames.append(frame)
        if frame.get("type") == "response" and frame.get("id") == "op1":
            break

    send_frame(client, request("shutdown", request_id="shutdown"))
    response = receive_frame(client)
    assert response is not None
    assert response["ok"] is True
    thread.join(timeout=2)
    client.close()
    server.close()
    assert not thread.is_alive()
    return frames


def outcome(**kwargs):
    values = dict(
        success=True,
        execution_count=1,
        stdout="",
        stderr="",
        displays=[],
        final_result=None,
        error=None,
        stdout_truncated_bytes=0,
        stderr_truncated_bytes=0,
    )
    values.update(kwargs)
    return ExecutionOutcome(**values)


def test_output_limits_require_positive_values_and_event_below_frame_limit():
    frame_limit = 8 * 1024 * 1024
    with pytest.raises(ValueError):
        OutputLimits(max_event_bytes=0).validate(max_frame_bytes=frame_limit)
    with pytest.raises(ValueError):
        OutputLimits(max_event_bytes=frame_limit).validate(max_frame_bytes=frame_limit)


def test_worker_json_integer_range_matches_rust_serde_json_value():
    assert encode_json({"value": SERDE_JSON_MIN_INTEGER})
    assert encode_json({"value": SERDE_JSON_MAX_INTEGER})

    with pytest.raises(ValueError, match="Rust serde_json range"):
        encode_json({"value": SERDE_JSON_MIN_INTEGER - 1})
    with pytest.raises(ValueError, match="Rust serde_json range"):
        encode_json({"nested": [SERDE_JSON_MAX_INTEGER + 1]})


def test_worker_json_object_keys_are_not_silently_coerced():
    assert encode_json({"nested": {"1": "preserved"}})

    for invalid in ({1: "integer"}, {False: "boolean"}, {None: "null"}):
        with pytest.raises(ValueError, match="object keys must be exact strings"):
            encode_json({"nested": invalid})


def test_invalid_rich_output_fails_operation_with_complete_lifecycle():
    limits = OutputLimits(
        max_event_bytes=2048,
        max_operation_bytes=4096,
        max_blob_bytes=512,
    )
    display = MimeBundle(
        data={"application/json": {"too_large": SERDE_JSON_MAX_INTEGER + 1}}
    )
    frames = run_execute(outcome(displays=[display]), limits)

    error = next(frame for frame in frames if frame.get("event") == "error")
    assert error["payload"]["ename"] == "CoKernelOutputTransportError"
    assert error["payload"]["evalue"] == (
        "JSON integer is outside the Rust serde_json range"
    )

    finished = next(
        frame for frame in frames if frame.get("event") == "execution_finished"
    )
    assert finished["payload"]["status"] == "FAILED"
    assert finished["payload"]["output_truncated"] is True
    assert "invalid_json" in finished["payload"]["output_truncation_reasons"]

    response = next(
        frame
        for frame in frames
        if frame.get("type") == "response" and frame.get("id") == "op1"
    )
    assert response["ok"] is True
    assert response["result"]["status"] == "FAILED"
    assert response["result"]["output_truncated"] is True
    assert response["result"]["output_truncation_reasons"] == finished["payload"][
        "output_truncation_reasons"
    ]


def test_non_string_rich_output_key_fails_without_wire_coercion():
    limits = OutputLimits(
        max_event_bytes=2048,
        max_operation_bytes=4096,
        max_blob_bytes=512,
    )
    display = MimeBundle(data={"application/json": {1: "must-not-become-string-one"}})
    frames = run_execute(outcome(displays=[display]), limits)

    error = next(frame for frame in frames if frame.get("event") == "error")
    assert error["payload"]["ename"] == "CoKernelOutputTransportError"
    assert error["payload"]["evalue"] == "JSON object keys must be exact strings"

    finished = next(
        frame for frame in frames if frame.get("event") == "execution_finished"
    )
    assert finished["payload"]["status"] == "FAILED"
    assert finished["payload"]["output_truncated"] is True
    assert "invalid_json" in finished["payload"]["output_truncation_reasons"]

    response = next(
        frame
        for frame in frames
        if frame.get("type") == "response" and frame.get("id") == "op1"
    )
    assert response["ok"] is True
    assert response["result"]["status"] == "FAILED"
    assert response["result"]["output_truncation_reasons"] == finished["payload"][
        "output_truncation_reasons"
    ]


def test_stream_event_is_truncated_and_records_metadata():
    limits = OutputLimits(
        max_event_bytes=512,
        max_operation_bytes=4096,
        max_blob_bytes=256,
    )
    frames = run_execute(outcome(stdout="x" * 5000), limits)
    stdout = next(frame for frame in frames if frame.get("event") == "stdout")
    encoded_len = len(
        json.dumps(stdout, ensure_ascii=False, separators=(",", ":")).encode("utf-8")
    )
    assert encoded_len <= limits.max_event_bytes
    assert len(stdout["payload"]["text"]) < 5000
    assert stdout["payload"]["truncation"]["truncated"] is True
    assert "event_limit" in stdout["payload"]["truncation"]["reasons"]

    finished = next(
        frame for frame in frames if frame.get("event") == "execution_finished"
    )
    assert finished["payload"]["output_truncated"] is True
    assert finished["payload"]["output_omitted_bytes"] > 0

    response = next(
        frame
        for frame in frames
        if frame.get("type") == "response" and frame.get("id") == "op1"
    )
    assert response["result"]["output_truncated"] == finished["payload"][
        "output_truncated"
    ]
    assert response["result"]["output_omitted_bytes"] == finished["payload"][
        "output_omitted_bytes"
    ]
    assert response["result"]["output_truncation_reasons"] == finished["payload"][
        "output_truncation_reasons"
    ]


def test_engine_stream_capture_omission_is_exposed_when_no_text_remains():
    limits = OutputLimits(
        max_event_bytes=1024,
        max_operation_bytes=4096,
        max_blob_bytes=256,
    )
    frames = run_execute(
        outcome(stdout="", stdout_truncated_bytes=1234),
        limits,
    )
    stdout = next(frame for frame in frames if frame.get("event") == "stdout")
    assert stdout["payload"]["text"] == ""
    assert stdout["payload"]["truncation"]["omitted_bytes"] == 1234
    assert stdout["payload"]["truncation"]["reasons"] == ["stream_capture_limit"]


def test_oversized_blob_mime_is_omitted_with_explicit_reason():
    limits = OutputLimits(
        max_event_bytes=2048,
        max_operation_bytes=4096,
        max_blob_bytes=128,
    )
    display = MimeBundle(
        data={"text/plain": "preview", "image/png": "A" * 1000}
    )
    frames = run_execute(outcome(displays=[display]), limits)
    event = next(frame for frame in frames if frame.get("event") == "display_data")
    assert event["payload"]["data"]["text/plain"] == "preview"
    assert "image/png" not in event["payload"]["data"]
    assert "blob_limit" in event["payload"]["truncation"]["reasons"]


def test_operation_budget_bounds_later_output_and_reports_truncation():
    limits = OutputLimits(
        max_event_bytes=700,
        max_operation_bytes=750,
        max_blob_bytes=256,
    )
    displays = [
        MimeBundle(data={"text/plain": "a" * 300}),
        MimeBundle(data={"text/plain": "b" * 300}),
    ]
    frames = run_execute(outcome(displays=displays), limits)
    output_events = [frame for frame in frames if frame.get("event") == "display_data"]
    assert len(output_events) == 2
    assert output_events[1]["payload"]["data"] == {}
    assert "operation_limit" in output_events[1]["payload"]["truncation"]["reasons"]

    finished = next(
        frame for frame in frames if frame.get("event") == "execution_finished"
    )
    assert finished["payload"]["output_truncated"] is True
    assert "operation_limit" in finished["payload"]["output_truncation_reasons"]

    response = next(
        frame
        for frame in frames
        if frame.get("type") == "response" and frame.get("id") == "op1"
    )
    assert response["result"]["output_truncation_reasons"] == finished["payload"][
        "output_truncation_reasons"
    ]


def test_handshake_advertises_output_contract_limits():
    limits = OutputLimits(
        max_event_bytes=1024,
        max_operation_bytes=4096,
        max_blob_bytes=512,
    )
    server, client = socket.socketpair()
    loop = WorkerLoop("s1", FakeEngine(outcome()), output_limits=limits)
    thread = threading.Thread(target=loop.run, args=(server,), daemon=True)
    thread.start()
    ready = receive_frame(client)
    assert ready is not None
    assert ready["event"] == "ready"

    send_frame(client, request("handshake", request_id="h"))
    response = receive_frame(client)
    assert response is not None
    assert response["result"]["output_limits"] == {
        "max_event_bytes": 1024,
        "max_operation_bytes": 4096,
        "max_blob_bytes": 512,
    }

    send_frame(client, request("shutdown", request_id="s"))
    shutdown = receive_frame(client)
    assert shutdown is not None
    assert shutdown["ok"] is True
    thread.join(timeout=2)
    client.close()
    server.close()
