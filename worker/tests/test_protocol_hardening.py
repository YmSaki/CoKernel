from __future__ import annotations

import socket
import struct
import threading

import pytest

from cokernel_worker.protocol import (
    INVALID_REQUEST_ID,
    MAX_ERROR_SUMMARY_CHARS,
    MAX_METHOD_CHARS,
    MAX_REQUEST_ID_CHARS,
    PROTOCOL_V1,
    WorkerLoop,
    WorkerProtocolError,
    _bounded_text,
    receive_frame,
    send_frame,
)


def request(method: str, payload: object = None, request_id: str = "r1"):
    return {
        "protocol": PROTOCOL_V1,
        "type": "request",
        "id": request_id,
        "session_id": "s1",
        "method": method,
        "payload": {} if payload is None else payload,
    }


def start_loop():
    server, client = socket.socketpair()
    thread = threading.Thread(target=WorkerLoop("s1").run, args=(server,), daemon=True)
    thread.start()
    ready = receive_frame(client)
    assert ready is not None and ready["event"] == "ready"
    return server, client, thread


def receive_response(client, request_id: str):
    while True:
        frame = receive_frame(client)
        assert frame is not None
        if frame.get("type") == "response" and frame.get("id") == request_id:
            return frame


def assert_worker_survives(client):
    send_frame(client, request("ping", {}, "good"))
    good = receive_response(client, "good")
    assert good["ok"] is True


def stop_loop(server, client, thread):
    send_frame(client, request("shutdown", {}, "shutdown"))
    response = receive_response(client, "shutdown")
    assert response["ok"] is True
    thread.join(timeout=2)
    client.close()
    server.close()
    assert not thread.is_alive()


def test_worker_send_rejects_non_finite_json_numbers() -> None:
    server, client = socket.socketpair()
    try:
        with pytest.raises(WorkerProtocolError, match="worker payload is not valid JSON"):
            send_frame(client, {"value": float("nan")})
    finally:
        client.close()
        server.close()


@pytest.mark.parametrize("constant", [b"NaN", b"Infinity", b"-Infinity"])
def test_worker_receive_rejects_non_standard_json_constants(constant: bytes) -> None:
    server, client = socket.socketpair()
    try:
        payload = b'{"value":' + constant + b"}"
        client.sendall(struct.pack(">I", len(payload)) + payload)
        with pytest.raises(WorkerProtocolError, match="frame is not valid UTF-8 JSON"):
            receive_frame(server)
    finally:
        client.close()
        server.close()


@pytest.mark.parametrize(
    "payload",
    [
        b'\xef\xbb\xbf{"value":1}',
        '{"value":1}'.encode("utf-16"),
    ],
    ids=["utf8-bom", "utf16"],
)
def test_worker_receive_requires_plain_utf8_json(payload: bytes) -> None:
    server, client = socket.socketpair()
    try:
        client.sendall(struct.pack(">I", len(payload)) + payload)
        with pytest.raises(WorkerProtocolError, match="frame is not valid UTF-8 JSON"):
            receive_frame(server)
    finally:
        client.close()
        server.close()


@pytest.mark.parametrize("payload", [None, [], "", 0, False])
def test_worker_rejects_explicit_non_object_payload_without_terminating(payload) -> None:
    server, client, thread = start_loop()
    try:
        frame = request("ping", {}, "bad")
        frame["payload"] = payload
        send_frame(client, frame)
        response = receive_response(client, "bad")
        assert response["ok"] is False
        assert response["error"]["summary"] == "request payload must be an object"
        assert_worker_survives(client)
    finally:
        stop_loop(server, client, thread)


def test_worker_bounds_invalid_request_id_and_keeps_running() -> None:
    server, client, thread = start_loop()
    try:
        send_frame(client, request("ping", {}, "x" * (MAX_REQUEST_ID_CHARS + 1)))
        response = receive_response(client, INVALID_REQUEST_ID)
        assert response["ok"] is False
        assert response["error"]["summary"] == "request id exceeds maximum length"
        assert_worker_survives(client)
    finally:
        stop_loop(server, client, thread)


def test_worker_rejects_oversized_method_and_keeps_running() -> None:
    server, client, thread = start_loop()
    try:
        send_frame(client, request("x" * (MAX_METHOD_CHARS + 1), {}, "bad-method"))
        response = receive_response(client, "bad-method")
        assert response["ok"] is False
        assert response["error"]["summary"] == "request method exceeds maximum length"
        assert_worker_survives(client)
    finally:
        stop_loop(server, client, thread)


def test_execute_operation_id_must_match_request_id_and_keeps_running() -> None:
    server, client, thread = start_loop()
    try:
        send_frame(
            client,
            request(
                "execute",
                {
                    "operation_id": "different-operation",
                    "cell_id": "cell-1",
                    "source": "should_not_run = 99",
                    "origin": "HUMAN",
                },
                "execute-1",
            ),
        )
        response = receive_response(client, "execute-1")
        assert response["ok"] is False
        assert (
            response["error"]["summary"]
            == "execute.operation_id must match request id"
        )

        send_frame(
            client,
            request("get_variable", {"name": "should_not_run"}, "inspect-after-reject"),
        )
        inspection = receive_response(client, "inspect-after-reject")
        assert inspection["ok"] is False
        assert inspection["error"]["summary"] == "variable not found: should_not_run"
        assert_worker_survives(client)
    finally:
        stop_loop(server, client, thread)


def test_error_summary_is_unicode_safely_bounded() -> None:
    summary = _bounded_text("界" * (MAX_ERROR_SUMMARY_CHARS + 10), MAX_ERROR_SUMMARY_CHARS)
    assert len(summary) == MAX_ERROR_SUMMARY_CHARS
    assert summary.endswith("…")
