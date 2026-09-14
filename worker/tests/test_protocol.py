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


def start_loop(*, heartbeat_interval_seconds: float = 2.0):
    server, client = socket.socketpair()
    thread = threading.Thread(
        target=WorkerLoop(
            "s1", heartbeat_interval_seconds=heartbeat_interval_seconds
        ).run,
        args=(server,),
        daemon=True,
    )
    thread.start()
    ready = receive_frame(client)
    assert ready is not None
    assert ready["event"] == "ready"
    return server, client, thread, ready


def receive_response(client, request_id: str):
    while True:
        frame = receive_frame(client)
        assert frame is not None
        if frame.get("type") == "response" and frame.get("id") == request_id:
            return frame


def stop_loop(client, thread):
    send_frame(client, request("shutdown", request_id="shutdown"))
    response = receive_response(client, "shutdown")
    assert response["ok"] is True
    thread.join(timeout=2)
    client.close()
    assert not thread.is_alive()


def test_worker_protocol_persists_namespace_and_supports_safe_inspection() -> None:
    server, client, thread, _ = start_loop()
    try:
        send_frame(
            client,
            request(
                "execute",
                {"operation_id": "op1", "cell_id": "c1", "source": "x = 123"},
                "op1",
            ),
        )
        frames = []
        while True:
            frame = receive_frame(client)
            assert frame is not None
            frames.append(frame)
            if frame.get("type") == "response" and frame.get("id") == "op1":
                break
        assert frames[-1]["result"]["status"] == "SUCCEEDED"

        send_frame(client, request("get_variable", {"name": "x"}, "g1"))
        value = receive_response(client, "g1")
        assert value["ok"] is True
        assert value["result"]["value"] == 123
        assert value["result"]["supported"] is True

        send_frame(
            client,
            request(
                "execute",
                {"operation_id": "op2", "cell_id": "c2", "source": "x + 1"},
                "op2",
            ),
        )
        frames = []
        while True:
            frame = receive_frame(client)
            assert frame is not None
            frames.append(frame)
            if frame.get("type") == "response" and frame.get("id") == "op2":
                break
        result_events = [
            frame for frame in frames if frame.get("event") == "execute_result"
        ]
        assert result_events
        assert result_events[0]["payload"]["data"]["text/plain"] == "124"
    finally:
        stop_loop(client, thread)
        server.close()


def test_worker_rejects_wrong_session_without_terminating() -> None:
    server, client, thread, _ = start_loop()
    try:
        bad = request("ping", request_id="bad")
        bad["session_id"] = "other"
        send_frame(client, bad)
        response = receive_response(client, "bad")
        assert response["ok"] is False

        send_frame(client, request("ping", request_id="good"))
        response = receive_response(client, "good")
        assert response["ok"] is True
    finally:
        stop_loop(client, thread)
        server.close()


def test_output_normalization_failure_completes_operation_and_worker_survives() -> None:
    server, client, thread, _ = start_loop()
    try:
        send_frame(
            client,
            request(
                "execute",
                {
                    "operation_id": "bad-output",
                    "cell_id": "bad-output-cell",
                    "source": (
                        "from IPython.display import display\n"
                        "display({'image/svg+xml': b'<svg>\\xff</svg>'}, raw=True)\n"
                        "marker = 41"
                    ),
                },
                "bad-output",
            ),
        )

        frames = []
        while True:
            frame = receive_frame(client)
            assert frame is not None
            frames.append(frame)
            if frame.get("type") == "response" and frame.get("id") == "bad-output":
                break

        transaction_frames = [
            frame
            for frame in frames
            if frame.get("event") != "heartbeat"
        ]
        assert [
            frame.get("event") or frame.get("type") for frame in transaction_frames
        ] == ["execution_started", "error", "execution_finished", "response"]

        error = transaction_frames[1]
        assert error["payload"]["ename"] == "CoKernelOutputNormalizationError"
        finished = transaction_frames[2]
        assert finished["payload"]["status"] == "FAILED"
        response = transaction_frames[3]
        assert response["ok"] is True
        assert response["result"]["status"] == "FAILED"

        # The namespace mutation happened before output normalization failed and
        # must remain usable in the same persistent worker Session.
        send_frame(
            client,
            request(
                "execute",
                {
                    "operation_id": "after-output-error",
                    "cell_id": "after-output-error-cell",
                    "source": "marker + 1",
                },
                "after-output-error",
            ),
        )
        frames = []
        while True:
            frame = receive_frame(client)
            assert frame is not None
            frames.append(frame)
            if (
                frame.get("type") == "response"
                and frame.get("id") == "after-output-error"
            ):
                break
        result = next(frame for frame in frames if frame.get("event") == "execute_result")
        assert result["payload"]["data"]["text/plain"] == "42"
        assert frames[-1]["result"]["status"] == "SUCCEEDED"
    finally:
        stop_loop(client, thread)
        server.close()


def test_nameless_exception_completes_operation_and_worker_survives() -> None:
    server, client, thread, _ = start_loop()
    try:
        send_frame(
            client,
            request(
                "execute",
                {
                    "operation_id": "nameless-error",
                    "cell_id": "nameless-error-cell",
                    "source": (
                        "class NamelessError(Exception):\n"
                        "    pass\n"
                        "NamelessError.__name__ = ''\n"
                        "marker = 41\n"
                        "raise NamelessError('boom')"
                    ),
                },
                "nameless-error",
            ),
        )

        frames = []
        while True:
            frame = receive_frame(client)
            assert frame is not None
            frames.append(frame)
            if (
                frame.get("type") == "response"
                and frame.get("id") == "nameless-error"
            ):
                break

        transaction_frames = [
            frame for frame in frames if frame.get("event") != "heartbeat"
        ]
        assert [
            frame.get("event") or frame.get("type") for frame in transaction_frames
        ] == ["execution_started", "error", "execution_finished", "response"]
        assert transaction_frames[1]["payload"]["ename"] == "Exception"
        assert transaction_frames[1]["payload"]["evalue"] == "boom"
        assert transaction_frames[2]["payload"]["status"] == "FAILED"
        assert transaction_frames[3]["result"]["status"] == "FAILED"

        send_frame(
            client,
            request(
                "execute",
                {
                    "operation_id": "after-nameless-error",
                    "cell_id": "after-nameless-error-cell",
                    "source": "marker + 1",
                },
                "after-nameless-error",
            ),
        )
        frames = []
        while True:
            frame = receive_frame(client)
            assert frame is not None
            frames.append(frame)
            if (
                frame.get("type") == "response"
                and frame.get("id") == "after-nameless-error"
            ):
                break
        result = next(frame for frame in frames if frame.get("event") == "execute_result")
        assert result["payload"]["data"]["text/plain"] == "42"
        assert frames[-1]["result"]["status"] == "SUCCEEDED"
    finally:
        stop_loop(client, thread)
        server.close()


def test_worker_emits_heartbeat_while_execution_is_running() -> None:
    server, client, thread, ready = start_loop(heartbeat_interval_seconds=0.02)
    try:
        assert ready["payload"]["heartbeat_interval_ms"] == 20
        send_frame(
            client,
            request(
                "execute",
                {
                    "operation_id": "slow-op",
                    "cell_id": "slow-cell",
                    "source": "import time\ntime.sleep(0.12)\n42",
                },
                "slow-op",
            ),
        )
        saw_heartbeat = False
        while True:
            frame = receive_frame(client)
            assert frame is not None
            if frame.get("event") == "heartbeat":
                saw_heartbeat = True
                assert type(frame["payload"]["monotonic_ns"]) is int
            if frame.get("type") == "response" and frame.get("id") == "slow-op":
                break
        assert saw_heartbeat
    finally:
        stop_loop(client, thread)
        server.close()


def test_handshake_advertises_heartbeat_contract() -> None:
    server, client, thread, _ = start_loop(heartbeat_interval_seconds=0.02)
    try:
        send_frame(client, request("handshake", request_id="handshake"))
        response = receive_response(client, "handshake")
        assert response["ok"] is True
        assert response["result"]["heartbeat_interval_ms"] == 20
    finally:
        stop_loop(client, thread)
        server.close()


@pytest.mark.parametrize(
    "heartbeat_interval_seconds",
    [0, -1, float("inf"), float("nan"), True],
)
def test_worker_rejects_invalid_heartbeat_interval(heartbeat_interval_seconds) -> None:
    with pytest.raises(ValueError, match="heartbeat_interval_seconds"):
        WorkerLoop("s1", heartbeat_interval_seconds=heartbeat_interval_seconds)
