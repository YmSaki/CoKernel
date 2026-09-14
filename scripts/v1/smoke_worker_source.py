#!/usr/bin/env python3
"""Source-tree worker subprocess smoke for constrained local verification.

This does not replace check-worker.sh: the canonical gate still requires the uv
Project/overlay path. It proves the checked source package itself can run as a
real Linux/WSL subprocess over a Supervisor-owned AF_UNIX socket.
"""
from __future__ import annotations

import json
import os
from pathlib import Path
import socket
import struct
import subprocess
import sys
import tempfile
from typing import Any

ROOT = Path(__file__).resolve().parents[2]
WORKER_SRC = ROOT / "worker" / "src"
PROTOCOL = 1


def send_frame(sock: socket.socket, message: dict[str, Any]) -> None:
    payload = json.dumps(message, ensure_ascii=False, separators=(",", ":")).encode("utf-8")
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
        print("[worker-source-smoke] SKIP: AF_UNIX requires Linux/WSL")
        return 0

    with tempfile.TemporaryDirectory(prefix="cokernel-worker-source-") as tmp:
        session_id = "source-smoke-session"
        socket_path = Path(tmp) / "worker.sock"
        listener = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
        listener.bind(str(socket_path))
        listener.listen(1)
        env = os.environ.copy()
        env["PYTHONPATH"] = str(WORKER_SRC) + os.pathsep + env.get("PYTHONPATH", "")
        process = subprocess.Popen(
            [sys.executable, "-m", "cokernel_worker", "--socket", str(socket_path), "--session-id", session_id],
            cwd=ROOT,
            env=env,
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
            assert ready["event"] == "ready" and ready["session_id"] == session_id
            assert ready["payload"]["pid"] == process.pid

            send_frame(conn, request(session_id, "handshake", "handshake"))
            handshake = read_until_response(conn, "handshake")[-1]
            assert handshake["ok"] is True
            assert {"execute", "inspect_variables", "get_variable", "reset", "shutdown"} <= set(handshake["result"]["capabilities"])

            send_frame(conn, request(session_id, "set", "execute", {"operation_id": "set", "source": "x = 123"}))
            assert read_until_response(conn, "set")[-1]["result"]["status"] == "SUCCEEDED"
            send_frame(conn, request(session_id, "get", "get_variable", {"name": "x"}))
            assert read_until_response(conn, "get")[-1]["result"]["value"] == 123

            send_frame(conn, request(session_id, "err", "execute", {"operation_id": "err", "source": "raise ValueError('boom')"}))
            failed = read_until_response(conn, "err")
            assert any(frame.get("event") == "error" for frame in failed)
            assert failed[-1]["result"]["status"] == "FAILED"

            send_frame(conn, request(session_id, "after", "execute", {"operation_id": "after", "source": "x + 1"}))
            after = read_until_response(conn, "after")
            assert any(frame.get("event") == "execute_result" and frame["payload"]["data"]["text/plain"] == "124" for frame in after)

            send_frame(conn, request(session_id, "reset", "reset"))
            assert read_until_response(conn, "reset")[-1]["result"]["reset"] is True
            send_frame(conn, request(session_id, "gone", "get_variable", {"name": "x"}))
            assert read_until_response(conn, "gone")[-1]["ok"] is False

            send_frame(conn, request(session_id, "stop", "shutdown"))
            assert read_until_response(conn, "stop")[-1]["result"]["shutdown"] is True
            process.wait(timeout=10)
            if process.returncode != 0:
                raise RuntimeError(process.stderr.read() if process.stderr else "worker failed")
        finally:
            if conn is not None:
                conn.close()
            listener.close()
            if process.poll() is None:
                process.kill()
                process.wait(timeout=5)

    print("[worker-source-smoke] PASS")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
