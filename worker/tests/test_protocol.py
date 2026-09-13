from __future__ import annotations

import socket
import threading

from IPython.core.interactiveshell import InteractiveShell
import pytest

from cokernel_worker.protocol import PROTOCOL_V1, WorkerLoop, receive_frame, send_frame


@pytest.fixture(autouse=True)
def isolated_ipython_singleton():
    InteractiveShell.clear_instance()
    yield
    InteractiveShell.clear_instance()


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
    thread = threading.Thread(
        target=WorkerLoop("s1").run,
        args=(server,),
        daemon=True,
    )
    thread.start()
    ready = receive_frame(client)
    assert ready is not None
    assert ready["event"] == "ready"
    return server, client, thread


def stop_loop(client, thread):
    send_frame(client, request("shutdown", request_id="shutdown"))
    response = receive_frame(client)
    assert response is not None
    assert response["ok"] is True
    thread.join(timeout=2)
    client.close()
    assert not thread.is_alive()


def test_worker_protocol_persists_namespace_and_supports_safe_inspection() -> None:
    server, client, thread = start_loop()
    try:
        send_frame(
            client,
            request(
                "execute",
                {"operation_id": "op1", "cell_id": "c1", "source": "x = 123"},
                "e1",
            ),
        )
        frames = []
        while True:
            frame = receive_frame(client)
            assert frame is not None
            frames.append(frame)
            if frame.get("type") == "response" and frame.get("id") == "e1":
                break
        assert frames[-1]["result"]["status"] == "SUCCEEDED"

        send_frame(client, request("get_variable", {"name": "x"}, "g1"))
        value = receive_frame(client)
        assert value is not None
        assert value["ok"] is True
        assert value["result"]["value"] == 123
        assert value["result"]["supported"] is True

        send_frame(
            client,
            request(
                "execute",
                {"operation_id": "op2", "cell_id": "c2", "source": "x + 1"},
                "e2",
            ),
        )
        frames = []
        while True:
            frame = receive_frame(client)
            assert frame is not None
            frames.append(frame)
            if frame.get("type") == "response" and frame.get("id") == "e2":
                break
        result_events = [frame for frame in frames if frame.get("event") == "execute_result"]
        assert result_events
        assert result_events[0]["payload"]["data"]["text/plain"] == "124"
    finally:
        stop_loop(client, thread)
        server.close()


def test_worker_rejects_wrong_session_without_terminating() -> None:
    server, client, thread = start_loop()
    try:
        bad = request("ping", request_id="bad")
        bad["session_id"] = "other"
        send_frame(client, bad)
        response = receive_frame(client)
        assert response is not None
        assert response["ok"] is False

        send_frame(client, request("ping", request_id="good"))
        response = receive_frame(client)
        assert response is not None
        assert response["ok"] is True
    finally:
        stop_loop(client, thread)
        server.close()
