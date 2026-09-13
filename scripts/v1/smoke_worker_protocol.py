from __future__ import annotations

import json
import os
from pathlib import Path
import socket
import struct
import subprocess
import tempfile
from typing import Any


ROOT = Path(__file__).resolve().parents[2]
WORKER = ROOT / "worker"
PROTOCOL = 1


def send_frame(sock: socket.socket, message: dict[str, Any]) -> None:
    payload = json.dumps(message, ensure_ascii=False, separators=(",", ":")).encode(
        "utf-8"
    )
    sock.sendall(struct.pack(">I", len(payload)) + payload)


def recv_exact(sock: socket.socket, size: int) -> bytes:
    data = bytearray()
    while len(data) < size:
        chunk = sock.recv(size - len(data))
        if not chunk:
            raise RuntimeError("worker disconnected mid-frame")
        data.extend(chunk)
    return bytes(data)


def recv_frame(sock: socket.socket) -> dict[str, Any]:
    size = struct.unpack(">I", recv_exact(sock, 4))[0]
    return json.loads(recv_exact(sock, size))


def request(session_id: str, request_id: str, method: str, payload=None):
    return {
        "protocol": PROTOCOL,
        "type": "request",
        "id": request_id,
        "session_id": session_id,
        "method": method,
        "payload": payload or {},
    }


def read_until_response(sock: socket.socket, request_id: str) -> list[dict[str, Any]]:
    frames: list[dict[str, Any]] = []
    while True:
        frame = recv_frame(sock)
        frames.append(frame)
        if frame.get("type") == "response" and frame.get("id") == request_id:
            return frames


def main() -> int:
    if os.name != "posix" or not hasattr(socket, "AF_UNIX"):
        print("[worker-protocol-smoke] SKIP: Unix domain socket runtime runs in Linux/WSL")
        return 0

    with tempfile.TemporaryDirectory(prefix="cokernel-worker-protocol-") as tmp:
        session_id = "00000000-0000-0000-0000-000000000001"
        socket_path = Path(tmp) / "worker.sock"
        listener = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
        listener.bind(str(socket_path))
        listener.listen(1)

        process = subprocess.Popen(
            [
                "uv",
                "run",
                "--project",
                str(WORKER),
                "--",
                "python",
                "-m",
                "cokernel_worker",
                "--socket",
                str(socket_path),
                "--session-id",
                session_id,
            ],
            cwd=ROOT,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            text=True,
        )
        conn = None
        try:
            listener.settimeout(10)
            conn, _ = listener.accept()
            conn.settimeout(10)
            ready = recv_frame(conn)
            assert ready["type"] == "event", ready
            assert ready["event"] == "ready", ready
            assert ready["session_id"] == session_id, ready
            assert int(ready["payload"]["pid"]) > 0, ready

            send_frame(
                conn,
                request(
                    session_id,
                    "exec-1",
                    "execute",
                    {
                        "operation_id": "op-1",
                        "cell_id": "cell-1",
                        "source": "x = 123",
                    },
                ),
            )
            frames = read_until_response(conn, "exec-1")
            assert frames[-1]["ok"] is True, frames
            assert frames[-1]["result"]["status"] == "SUCCEEDED", frames

            send_frame(
                conn,
                request(session_id, "get-1", "get_variable", {"name": "x"}),
            )
            frames = read_until_response(conn, "get-1")
            assert frames[-1]["result"]["value"] == 123, frames

            send_frame(
                conn,
                request(
                    session_id,
                    "exec-2",
                    "execute",
                    {
                        "operation_id": "op-2",
                        "cell_id": "cell-2",
                        "source": "x + 1",
                    },
                ),
            )
            frames = read_until_response(conn, "exec-2")
            result_events = [frame for frame in frames if frame.get("event") == "execute_result"]
            assert result_events, frames
            assert result_events[0]["payload"]["data"]["text/plain"] == "124", frames

            send_frame(conn, request(session_id, "stop-1", "shutdown"))
            frames = read_until_response(conn, "stop-1")
            assert frames[-1]["ok"] is True, frames
            process.wait(timeout=10)
            if process.returncode != 0:
                stderr = process.stderr.read() if process.stderr else ""
                raise RuntimeError(
                    f"worker exited with {process.returncode}; stderr={stderr!r}"
                )
        finally:
            if conn is not None:
                conn.close()
            listener.close()
            if process.poll() is None:
                process.kill()
                process.wait(timeout=5)

    print("[worker-protocol-smoke] PASS")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
