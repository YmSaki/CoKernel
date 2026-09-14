from __future__ import annotations

import json
import socket
import struct
import threading

import pytest

from cokernel_worker.protocol import (
    PROTOCOL_V1,
    WorkerLoop,
    WorkerProtocolError,
    receive_frame,
    send_frame,
)


def request(method: str, payload=None, request_id: str = "r1"):
    return {
        "protocol": PROTOCOL_V1,
        "type": "request",
        "id": request_id,
        "session_id": "s1",
        "method": method,
        "payload": payload or {},
    }


def start_loop():
    server, client = socket.socketpair()
    thread = threading.Thread(target=WorkerLoop("s1").run, args=(server,), daemon=True)
    thread.start()
    ready = receive_frame(client)
    assert ready is not None and ready["event"] == "ready"
    return server, client, thread


def receive_response(client: socket.socket, request_id: str):
    frames = []
    while True:
        frame = receive_frame(client)
        assert frame is not None
        frames.append(frame)
        if frame.get("type") == "response" and frame.get("id") == request_id:
            return frames


def stop_loop(server: socket.socket, client: socket.socket, thread: threading.Thread):
    send_frame(client, request("shutdown", request_id="shutdown"))
    assert receive_response(client, "shutdown")[-1]["ok"] is True
    thread.join(timeout=2)
    client.close()
    server.close()
    assert not thread.is_alive()


def test_send_frame_enforces_frame_size_before_write() -> None:
    server, client = socket.socketpair()
    try:
        with pytest.raises(WorkerProtocolError, match="frame exceeds maximum size"):
            send_frame(client, {"value": "x" * 100}, max_bytes=16)
        server.setblocking(False)
        with pytest.raises(BlockingIOError):
            server.recv(1)
    finally:
        client.close()
        server.close()


def test_receive_frame_rejects_oversized_declared_length_before_payload() -> None:
    server, client = socket.socketpair()
    try:
        client.sendall(struct.pack(">I", 17))
        with pytest.raises(WorkerProtocolError, match="frame exceeds maximum size"):
            receive_frame(server, max_bytes=16)
    finally:
        client.close()
        server.close()


@pytest.mark.parametrize(
    ("field", "value", "summary"),
    [
        ("protocol", 2, "unsupported worker protocol"),
        ("type", "event", "worker accepts request frames only"),
    ],
)
def test_invalid_envelope_fails_closed_and_worker_survives(
    field: str, value: object, summary: str
) -> None:
    server, client, thread = start_loop()
    try:
        bad = request("ping", request_id="bad")
        bad[field] = value
        send_frame(client, bad)
        response = receive_response(client, "bad")[-1]
        assert response["ok"] is False
        assert response["error"]["summary"] == summary

        send_frame(client, request("ping", request_id="good"))
        assert receive_response(client, "good")[-1]["result"] == {"pong": True}
    finally:
        stop_loop(server, client, thread)


def test_reset_discards_namespace_without_restarting_worker() -> None:
    server, client, thread = start_loop()
    try:
        send_frame(
            client,
            request("execute", {"operation_id": "set", "source": "x = 123"}, "set"),
        )
        assert receive_response(client, "set")[-1]["result"]["status"] == "SUCCEEDED"

        send_frame(client, request("reset", request_id="reset"))
        assert receive_response(client, "reset")[-1]["result"] == {"reset": True}

        send_frame(client, request("get_variable", {"name": "x"}, "get"))
        response = receive_response(client, "get")[-1]
        assert response["ok"] is False
        assert response["error"]["summary"] == "variable not found: x"
    finally:
        stop_loop(server, client, thread)


def test_stdout_stderr_events_are_bounded_and_lifecycle_count_matches() -> None:
    server, client, thread = start_loop()
    try:
        send_frame(
            client,
            request(
                "execute",
                {
                    "operation_id": "streams",
                    "source": (
                        "import sys\n"
                        "print('o' * 7000000)\n"
                        "print('e' * 7000000, file=sys.stderr)"
                    ),
                },
                "streams",
            ),
        )
        frames = receive_response(client, "streams")
        outputs = [f for f in frames if f.get("event") in {"stdout", "stderr"}]
        assert [f["event"] for f in outputs] == ["stdout", "stderr"]
        assert [f["payload"]["sequence"] for f in outputs] == [1, 2]
        for frame in outputs:
            assert len(json.dumps(frame, ensure_ascii=False, separators=(",", ":")).encode()) < 8 * 1024 * 1024
            assert frame["payload"]["truncation"]["truncated"] is True
            assert "stream_capture_limit" in frame["payload"]["truncation"]["reasons"]
        finished = next(f for f in frames if f.get("event") == "execution_finished")
        assert finished["payload"]["output_count"] == 2
        assert finished["payload"]["output_truncated"] is True
        assert finished["payload"]["output_omitted_bytes"] > 0
        response = frames[-1]
        assert response["result"]["output_truncation_reasons"] == finished["payload"]["output_truncation_reasons"]
    finally:
        stop_loop(server, client, thread)
