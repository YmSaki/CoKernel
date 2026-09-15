#!/usr/bin/env python3
"""Minimal fake uv/CoKernel worker used by Rust Session Supervisor component tests."""

import json
import os
import select
import socket
import struct
import sys
import time


def arg_after(flag: str) -> str:
    index = sys.argv.index(flag)
    return sys.argv[index + 1]


def recv_exact(sock: socket.socket, size: int) -> bytes:
    data = bytearray()
    while len(data) < size:
        chunk = sock.recv(size - len(data))
        if not chunk:
            raise RuntimeError("socket closed mid-frame")
        data.extend(chunk)
    return bytes(data)


def recv_frame(sock: socket.socket) -> dict:
    size = struct.unpack(">I", recv_exact(sock, 4))[0]
    return json.loads(recv_exact(sock, size))


def send_frame(sock: socket.socket, message: dict) -> None:
    payload = json.dumps(message, separators=(",", ":")).encode("utf-8")
    sock.sendall(struct.pack(">I", len(payload)) + payload)


def serve_cooperative(sock: socket.socket, session_id: str) -> None:
    sequence = 1
    while True:
        readable, _, _ = select.select([sock], [], [], 0.05)
        if readable:
            request = recv_frame(sock)
            if request.get("method") == "shutdown":
                raise SystemExit(0)
            raise RuntimeError(f"unsupported cooperative request: {request.get('method')!r}")
        send_frame(
            sock,
            {
                "protocol": 1,
                "type": "event",
                "session_id": session_id,
                "event": "heartbeat",
                "payload": {"monotonic_ns": sequence},
            },
        )
        sequence += 1


socket_path = arg_after("--socket")
session_id = arg_after("--session-id")
mode = os.path.basename(arg_after("--project"))
reported_pid = os.getpid() + 1 if mode == "bad-ready-pid" else os.getpid()

sock = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
sock.connect(socket_path)
send_frame(
    sock,
    {
        "protocol": 1,
        "type": "event",
        "session_id": session_id,
        "event": "ready",
        "payload": {
            "worker_version": "fake-1",
            "python_version": "fake-3",
            "ipython_version": "fake-9",
            "pid": reported_pid,
            "heartbeat_interval_ms": 50,
        },
    },
)

if mode == "bad-ready-pid":
    time.sleep(30)
    raise SystemExit(0)

handshake = recv_frame(sock)
assert handshake["method"] == "handshake"
capabilities = [
    "execute",
    "inspect_variables",
    "get_variable",
    "reset",
    "shutdown",
]
if mode == "bad-handshake":
    capabilities.remove("get_variable")

send_frame(
    sock,
    {
        "protocol": 1,
        "type": "response",
        "id": handshake["id"],
        "session_id": session_id,
        "ok": True,
        "result": {
            "protocol": 1,
            "worker_version": "fake-1",
            "capabilities": capabilities,
            "heartbeat_interval_ms": 50,
            "output_limits": {
                "max_event_bytes": 1024,
                "max_operation_bytes": 4096,
                "max_blob_bytes": 512,
            },
        },
    },
)

if mode in {"bad-handshake", "heartbeat-timeout"}:
    time.sleep(30)
elif mode == "healthy" or mode.startswith("lifecycle"):
    serve_cooperative(sock, session_id)
elif mode == "protocol-violation":
    # Give the Rust caller time to subscribe after ensure_primary returns.
    time.sleep(0.35)
    send_frame(
        sock,
        {
            "protocol": 1,
            "type": "response",
            "id": "stale-response",
            "session_id": session_id,
            "ok": True,
            "result": {"pong": True},
        },
    )
    time.sleep(30)
else:
    raise RuntimeError(f"unsupported fake worker mode: {mode}")
