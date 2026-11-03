"""Pinned real tunnel-client -> actual MCP -> primary worker, using a local control plane.

Pass the verified v0.0.14 executable path. No real credentials or external tunnel
are used. This is component/integration evidence, not ChatGPT remote acceptance.
"""
import http.server
import json
import os
from pathlib import Path
import queue
import signal
import socket
import struct
import sys
import tempfile
import threading
import time

from smoke_mcp_supervision import environment, gone
from smoke_notebook_runtime import Bridge, import_notebook, wait_operation, wait_package

KEY = "synthetic-local-tunnel-proof-key"
TUNNEL_ID = "tunnel_" + "0" * 32


class Plane(http.server.ThreadingHTTPServer):
    def __init__(self):
        super().__init__(("127.0.0.1", 0), Handler)
        self.commands = queue.Queue()
        self.responses = queue.Queue()
        self.offline = threading.Event()
        self.errors = queue.Queue()


class Handler(http.server.BaseHTTPRequestHandler):
    def answer(self, code, value=None):
        body = json.dumps(value).encode() if value is not None else b""
        self.send_response(code)
        self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(body)))
        self.end_headers()
        try:
            self.wfile.write(body)
        except (BrokenPipeError, ConnectionResetError):
            pass

    def authorized(self):
        if self.headers.get("Authorization") != "Bearer " + KEY:
            self.server.errors.put("missing synthetic provider authorization")
            self.answer(401)
            return False
        return True

    def do_GET(self):
        if not self.authorized():
            return
        if self.server.offline.is_set():
            self.answer(503)
        elif self.path.startswith(f"/v1/tunnels/{TUNNEL_ID}/poll?"):
            try:
                command = self.server.commands.get(timeout=.1)
                self.answer(200, {"commands": [command]})
            except queue.Empty:
                self.answer(204)
        elif self.path == f"/v1/tunnels/{TUNNEL_ID}":
            self.answer(200, {"id": TUNNEL_ID, "name": "local acceptance fixture"})
        else:
            self.server.errors.put("unexpected control plane route")
            self.answer(404)

    def do_POST(self):
        if not self.authorized():
            return
        length = int(self.headers.get("Content-Length", "0"))
        if length > 512 * 1024 or self.path != f"/v1/tunnels/{TUNNEL_ID}/response":
            self.server.errors.put("unexpected response route or bound")
            self.answer(400)
            return
        data = json.loads(self.rfile.read(length))
        if self.headers.get("X-Tunnel-Shard-Token") != "local-shard" or "shard_token" in data:
            self.server.errors.put("invalid shard correlation")
        self.server.responses.put(data)
        self.answer(200, {"status": "ok"})

    def log_message(self, *args):
        pass


def status_until(bridge, predicate, seconds=20):
    deadline = time.monotonic() + seconds
    while True:
        state = bridge.receive(bridge.send("runtime.get_tunnel_status"), timeout=2)
        assert bridge.receive(bridge.send("bridge.ping"), timeout=2)["pong"]
        if predicate(state):
            return state
        assert time.monotonic() < deadline, state
        time.sleep(.05)


def main():
    executable = Path(sys.argv[1]).resolve(strict=True)
    plane = Plane()
    thread = threading.Thread(target=plane.serve_forever, daemon=True)
    thread.start()
    try:
        with tempfile.TemporaryDirectory(prefix="ck-tunnel-") as tmp:
            directory = Path(tmp)
            # Trusted bootstrap fixture replaces only the provider URL. The product
            # RPC and production Config provide no URL/path override to clients.
            wrapper = directory / "tunnel"
            wrapper.write_text("#!/usr/bin/python3\nimport os,sys\na=sys.argv[1:]\n"
                "i=a.index('--control-plane.base-url')+1\n"
                f"a[i]='http://127.0.0.1:{plane.server_port}'\n"
                f"os.execv({str(executable)!r},[{str(executable)!r}]+a)\n")
            wrapper.chmod(0o700)
            env = environment(directory)
            env.update(COKERNEL_TUNNEL_EXECUTABLE=str(wrapper), COKERNEL_TUNNEL_STATE_DIR=tmp)
            bridge = Bridge(env, directory)
            try:
                assert bridge.rpc("runtime.get_tunnel_status")["state"] == "DISABLED"
                bridge.rpc("tunnel.start", tunnel_id=TUNNEL_ID, runtime_key=KEY)
                ready = status_until(bridge, lambda s: s["state"] == "CONNECTED")
                bearer = (directory / "bearer").read_text()

                sequence = 0
                def remote(method, params):
                    nonlocal sequence
                    sequence += 1
                    plane.commands.put({"request_id": str(sequence), "shard_token": "local-shard",
                        "command_type": "jsonrpc", "channel": "main", "response_timeout": "10s",
                        "headers": {"MCP-Protocol-Version": ["2025-11-25"]},
                        "jsonrpc": {"jsonrpc": "2.0", "id": sequence, "method": method, "params": params}})
                    result = plane.responses.get(timeout=15)
                    assert result["request_id"] == str(sequence) and result["resp_code"] == 200, result
                    assert result["resp_json"]["id"] == sequence, result
                    assert bearer not in json.dumps(result) and KEY not in json.dumps(result)
                    return result["resp_json"]["result"]

                initialized = remote("initialize", {"protocolVersion": "2025-11-25", "capabilities": {},
                    "clientInfo": {"name": "local-tunnel-proof", "version": "1"}})
                assert initialized["serverInfo"]["name"]
                assert len(remote("tools/list", {})["tools"]) == 15

                created = bridge.rpc("projects.create", name="tunnel-shared")
                project = wait_package(bridge, created["project_id"], created)["project"]["project_id"]
                source = "import os\nworker_pid=os.getpid()\nsecret_from_master=123456789"
                notebook = {"nbformat": 4, "nbformat_minor": 5, "metadata": {}, "cells": [
                    {"cell_type": "code", "id": "state", "metadata": {}, "execution_count": None, "outputs": [], "source": source},
                    {"cell_type": "code", "id": "survive", "metadata": {}, "execution_count": None, "outputs": [], "source": "secret_from_master += 1"}]}
                doc = import_notebook(bridge, project, "tunnel.ipynb", notebook)["document"]
                identity = {"project_id": project, "notebook_id": doc["notebook_id"]}
                session = bridge.rpc("sessions.ensure_primary", **identity)["session_id"]
                live = {"project_id": project, "session_id": session}

                def execute(cell):
                    revision = bridge.rpc("notebooks.get", **identity)["revision"]
                    accepted = bridge.rpc("sessions.execute_cell", **identity, session_id=session,
                        cell_id=cell, expected_revision=revision)
                    assert wait_operation(bridge, project, accepted["operation_id"])["execution_status"] == "SUCCEEDED"

                def variable(name):
                    result = remote("tools/call", {"name": "get_variable", "arguments": dict(live, name=name)})
                    assert not result["isError"], result
                    return json.loads(result["content"][0]["text"])["value"]

                execute("state")
                worker = bridge.rpc("sessions.get_variable", **live, name="worker_pid")["value"]
                assert variable("secret_from_master") == 123456789
                assert variable("worker_pid") == worker
                # Neither private control nor secret handoff is exposed via Unix MCP API.
                client = socket.socket(socket.AF_UNIX)
                client.settimeout(3)
                client.connect(str(directory / "runtime.sock"))
                request = json.dumps({"protocol": 1, "request_id": "deny", "method": "tunnel.start",
                    "params": {"tunnel_id": TUNNEL_ID, "runtime_key": KEY}}).encode()
                client.sendall(struct.pack(">I", len(request)) + request)
                def exact(size):
                    data = b""
                    while len(data) < size:
                        part = client.recv(size-len(data))
                        assert part
                        data += part
                    return data
                denied = json.loads(exact(struct.unpack(">I", exact(4))[0]))
                client.close()
                assert not denied["ok"] and KEY not in json.dumps(denied)

                # Real network failure: local compute continues before and after
                # remote status degrades; the client itself reconnects on recovery.
                plane.offline.set()
                execute("survive")
                degraded = status_until(bridge, lambda s: s["state"] == "DEGRADED", seconds=55)
                assert degraded["pid"] == ready["pid"]
                execute("survive")
                plane.offline.clear()
                restored = status_until(bridge, lambda s: s["state"] == "CONNECTED", seconds=35)
                assert restored["pid"] == ready["pid"]
                assert variable("secret_from_master") == 123456791

                # MCP replacement is a dependency change, not a tunnel failure.
                bridge.rpc("mcp.restart")
                replaced = status_until(bridge, lambda s: s["state"] == "CONNECTED" and s["mcp_generation"] != ready["mcp_generation"])
                gone(ready["pid"])
                assert replaced["consecutive_failures"] == 0
                assert variable("worker_pid") == worker
                os.kill(replaced["pid"], signal.SIGKILL)
                recovered = status_until(bridge, lambda s: s["state"] == "CONNECTED" and s["generation"] > replaced["generation"])
                gone(replaced["pid"])
                assert recovered["failures"][-1]["code"] == "CK-TUN-EXIT"
                assert variable("secret_from_master") == 123456791

                for pid in [worker, bridge.rpc("runtime.get_mcp_status")["pid"]]:
                    for field in ["environ", "cmdline"]:
                        data = Path(f"/proc/{pid}/{field}").read_bytes()
                        assert KEY.encode() not in data and bearer.encode() not in data
                args = Path(f"/proc/{recovered['pid']}/cmdline").read_bytes()
                assert KEY.encode() not in args and bearer.encode() not in args
                # A real credential does enter only the intended tunnel environment.
                tunnel_env = Path(f"/proc/{recovered['pid']}/environ").read_bytes()
                assert KEY.encode() in tunnel_env and bearer.encode() in tunnel_env
                epoch = bridge.rpc("tunnel.stop")["epoch"]
                status_until(bridge, lambda s: s["state"] == "DISABLED" and s["applied_epoch"] == epoch)
                gone(recovered["pid"])
                execute("survive")
                assert bridge.rpc("sessions.get_variable", **live, name="secret_from_master")["value"] == 123456792
                assert bridge.rpc("runtime.get_snapshot")["tunnel"]["state"] == "DISABLED"
                assert bridge.rpc("bridge.shutdown")["stopped"]
                assert bridge.process.wait(timeout=10) == 0
                gone(worker)
                assert KEY.encode() not in (directory / "runtime.stderr").read_bytes()
                while not bridge.events.empty():
                    data = json.dumps(bridge.events.get())
                    assert KEY not in data and bearer not in data
                assert plane.errors.empty(), list(plane.errors.queue)
                print("[tunnel-supervision-smoke] PASS: pinned actual client; local fake provider -> authenticated MCP -> same primary worker/value; provider disconnect/reconnect without worker loss; MCP replacement; process crash recovery; disable; shutdown; credential environment/argv/event boundaries. Actual Host secret handoff, managed identities and Web ChatGPT remain unverified.")
            finally:
                bridge.finish()
    finally:
        plane.shutdown()
        plane.server_close()
        thread.join(timeout=3)


if __name__ == "__main__":
    main()
