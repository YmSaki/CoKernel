"""Exercise the actual framed Runtime -> uv -> worker -> document path on Linux."""
from __future__ import annotations

import base64
import hashlib
import json
import os
from pathlib import Path
import queue
import signal
import struct
import subprocess
import tempfile
import threading
import time

ROOT = Path(__file__).resolve().parents[2]
FRAME_LIMIT = 8 * 1024 * 1024


class Bridge:
    def __init__(self, environment: dict[str, str], directory: Path, *, executable=None, process_options=None, observer=None):
        self.observer = observer
        self.stderr = (directory / "runtime.stderr").open("wb")
        self.process = subprocess.Popen(
            [str(executable or ROOT / "target/debug/cokernel-runtime"), "bridge", "--stdio"],
            stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=self.stderr,
            env=environment, cwd=ROOT,
            **(process_options or {}),
        )
        self.results: queue.Queue = queue.Queue()
        self.response_buffer = {}
        self.sequence = 0
        self.event_sequence = 0
        self.events = queue.Queue(maxsize=256)
        threading.Thread(target=self._read, daemon=True).start()

    def _exact(self, size: int) -> bytes:
        data = bytearray()
        while len(data) < size:
            part = self.process.stdout.read(size - len(data))
            if not part:
                raise EOFError("Runtime bridge closed")
            data.extend(part)
        return bytes(data)

    def _read(self):
        try:
            while True:
                length = struct.unpack(">I", self._exact(4))[0]
                if length > FRAME_LIMIT:
                    raise ValueError("Runtime emitted an oversized frame")
                value = json.loads(self._exact(length))
                if self.observer:
                    self.observer(value)
                if "event" in value:
                    assert value["sequence"] > self.event_sequence, value
                    self.event_sequence = value["sequence"]
                    if self.events.full():
                        self.events.get_nowait()
                    self.events.put_nowait(value)
                else:
                    self.results.put(value)
        except Exception as error:
            self.results.put(error)

    def send(self, method: str, **params):
        self.sequence += 1
        request_id = str(self.sequence)
        body = json.dumps({"protocol": 1, "request_id": request_id,
                           "method": method, "params": params}).encode()
        assert len(body) <= FRAME_LIMIT
        self.process.stdin.write(struct.pack(">I", len(body)) + body)
        self.process.stdin.flush()
        return request_id

    def receive(self, request_id, expected_error=None, timeout=40):
        deadline = time.monotonic() + timeout
        while request_id not in self.response_buffer:
            response = self.results.get(timeout=max(.01, deadline - time.monotonic()))
            if isinstance(response, Exception):
                raise response
            assert response["request_id"] not in self.response_buffer
            self.response_buffer[response["request_id"]] = response
            assert len(self.response_buffer) <= 64
        response = self.response_buffer.pop(request_id)
        assert response["request_id"] == request_id, response
        if expected_error:
            assert not response["ok"] and response["error"]["code"] == expected_error, response
            return response["error"]
        assert response["ok"], response
        return response["result"]

    def rpc(self, method: str, expected_error: str | None = None, **params):
        return self.receive(self.send(method, **params), expected_error=expected_error)

    def finish(self):
        # These processes belong only to this test Runtime. On an assertion
        # failure, also clean its worker descendants which own separate groups.
        descendants = []
        pending = [self.process.pid]
        seen = set()
        while pending and len(seen) < 256:
            pid = pending.pop()
            if pid in seen:
                continue
            seen.add(pid)
            try:
                stat = Path(f"/proc/{pid}/stat").read_text().rsplit(") ", 1)[1].split()
                descendants.append((pid, stat[19]))
                for children in Path(f"/proc/{pid}/task").glob("*/children"):
                    pending.extend(map(int, children.read_text().split()))
            except (OSError, IndexError):
                pass
        self.process.stdin.close()
        try:
            self.process.wait(timeout=5)
        except subprocess.TimeoutExpired:
            self.process.kill()
            self.process.wait(timeout=5)
        for pid, started in reversed(descendants):
            if pid == self.process.pid:
                continue
            try:
                actual = Path(f"/proc/{pid}/stat").read_text().rsplit(") ", 1)[1].split()[19]
                if actual == started:
                    os.kill(pid, signal.SIGKILL)
            except (OSError, IndexError):
                pass
        self.stderr.close()
        self.process.stdout.close()


def import_notebook(bridge: Bridge, project: str, path: str, notebook: dict):
    data = json.dumps(notebook, ensure_ascii=False).encode()
    ticket = bridge.rpc("notebooks.begin_import", project_id=project, target_path=path,
                        size_bytes=len(data), sha256=hashlib.sha256(data).hexdigest())
    size = ticket["accepted_chunk_size"]
    for offset in range(0, len(data), size):
        chunk = data[offset:offset + size]
        accepted = bridge.rpc("notebooks.import_chunk", project_id=project,
                              import_id=ticket["import_id"], offset=offset,
                              data_base64=base64.b64encode(chunk).decode())
        assert accepted["next_offset"] == offset + len(chunk)
    return bridge.rpc("notebooks.finish_import", project_id=project, import_id=ticket["import_id"])


def wait_operation(bridge: Bridge, project: str, operation: str):
    deadline = time.monotonic() + 25
    while time.monotonic() < deadline:
        status = bridge.rpc("operations.get", project_id=project, operation_id=operation)["result"]
        if status["state"] != "PENDING":
            return status
        time.sleep(0.02)
    raise TimeoutError("Runtime did not finish execution/persistence")


def wait_package(bridge: Bridge, project: str, operation: dict):
    assert operation["kind"] in {"PACKAGE", "PROJECT"}
    deadline = time.monotonic() + 90
    while time.monotonic() < deadline:
        result = bridge.rpc("operations.get", project_id=project,
                            operation_id=operation["operation_id"])["result"]
        if result["state"] != "RUNNING":
            assert result["state"] == "SUCCEEDED", result
            return result["project"]
        time.sleep(0.02)
    raise TimeoutError("Runtime did not finish package operation")


def main() -> int:
    if os.name == "nt":
        print("[notebook-runtime-smoke] SKIP: production Runtime runs in Linux/WSL")
        return 0
    import nbformat  # Test-only dependency, supplied by the canonical uv command.

    subprocess.run(["cargo", "build", "-q", "-p", "cokernel-runtime"], cwd=ROOT, check=True)
    with tempfile.TemporaryDirectory(prefix="ckrpc-") as temporary:
        directory = Path(temporary)
        environment = dict(os.environ)
        environment.update({
            "COKERNEL_PROJECTS_ROOT": str(directory / "projects"),
            "COKERNEL_PROJECT_REGISTRY": str(directory / "state/projects.json"),
            "COKERNEL_WORKER_PACKAGE": str(ROOT / "worker"),
            "COKERNEL_SESSION_SOCKET_DIR": str(directory / "s"),
            "COKERNEL_FAILURE_DIR": str(directory / "failures"),
        })
        bridge = Bridge(environment, directory)
        try:
            hello = bridge.rpc("bridge.hello", host_product_version="smoke", supported_protocols=[1])
            assert "notebooks" in hello["capabilities"] and "sessions" in hello["capabilities"]
            operation = bridge.rpc("projects.create", name="proof")
            record = wait_package(bridge, operation["project_id"], operation)
            project = record["project"]["project_id"]
            project_path = Path(record["project"]["root_path"])
            operation = bridge.rpc("projects.create", name="other")
            other = wait_package(bridge, operation["project_id"], operation)["project"]["project_id"]
            for invalid_data, error in [("!invalid", "CK-NB-IMPORT"), ("a" * 349529, "CK-NB-LIMIT")]:
                ticket = bridge.rpc("notebooks.begin_import", project_id=project,
                                    target_path="invalid.ipynb", size_bytes=1, sha256="0" * 64)
                chunk_args = dict(import_id=ticket["import_id"], offset=0, data_base64=invalid_data)
                bridge.rpc("notebooks.import_chunk", expected_error="CK-NB-NOT-FOUND", project_id=other, **chunk_args)
                # The owner still finds the upload after the wrong-Project call;
                # its own invalid chunk then aborts it and frees the target.
                bridge.rpc("notebooks.import_chunk", expected_error=error, project_id=project, **chunk_args)
                bridge.rpc("notebooks.finish_import", expected_error="CK-NB-NOT-FOUND",
                           project_id=project, import_id=ticket["import_id"])
                assert not (project_path / "invalid.ipynb").exists()
            wait_package(bridge, project, bridge.rpc("packages.add", project_id=project, packages=[str(ROOT / "tests/fixtures/python-package")]))
            notebook = json.loads((ROOT / "tests/fixtures/notebooks/compatibility.ipynb").read_text())
            notebook["cells"][1].update(source=(
                "import cokernel_fixture_package as p\n"
                "from IPython.display import display, HTML\n"
                "x = 123\nsecret_from_master = 123456789\n"
                "print(p.answer())\ndisplay(HTML('<b>runtime</b>'))\nx"
            ), execution_count=None, outputs=[])
            notebook["cells"].append({"id": "next", "cell_type": "code", "metadata": {},
                                      "source": "x + 1", "execution_count": None, "outputs": []})
            imported = import_notebook(bridge, project, "notebook.ipynb", notebook)
            notebook_id = imported["document"]["notebook_id"]
            assert imported["document"]["revision"] == 1
            session = bridge.rpc("sessions.ensure_primary", project_id=project, notebook_id=notebook_id)
            session_id = session["session_id"]
            args = dict(project_id=project, session_id=session_id, notebook_id=notebook_id,
                        cell_id="compute", expected_revision=1)
            bridge.rpc("sessions.execute_cell", expected_error="CK-RUN-BAD-PARAMS", source="wrong source", **args)
            bridge.rpc("sessions.execute_cell", expected_error="CK-RUN-BAD-PARAMS", origin="MCP", **args)
            conflict = bridge.rpc("sessions.execute_cell", expected_error="CK-NB-CONFLICT", **{**args, "expected_revision": 0})
            assert json.loads(conflict["detail"])["current_revision"] == 1
            operation = bridge.rpc("sessions.execute_cell", **args)
            result = wait_operation(bridge, project, operation["operation_id"])
            assert result["state"] == "PERSISTED" and result["execution_status"] == "SUCCEEDED", result
            snapshot = bridge.rpc("notebooks.get", project_id=project, notebook_id=notebook_id)
            outputs = snapshot["notebook"]["cells"][1]["outputs"]
            assert any("42" in output.get("text", "") for output in outputs), outputs
            assert any(output.get("data", {}).get("text/html") == "<b>runtime</b>" for output in outputs)
            operation = bridge.rpc("sessions.execute_cell", **{**args, "cell_id": "next", "expected_revision": snapshot["revision"]})
            result = wait_operation(bridge, project, operation["operation_id"])
            assert result["state"] == "PERSISTED", result
            snapshot = bridge.rpc("notebooks.get", project_id=project, notebook_id=notebook_id)
            assert snapshot["notebook"]["cells"][3]["outputs"][0]["data"]["text/plain"] == "124"
            persisted = nbformat.read(project_path / "notebook.ipynb", as_version=4)
            nbformat.validate(persisted)
            # nbformat joins string-list sources when reading. Compare raw JSON
            # separately to prove that CoKernel preserves the original shape.
            raw_saved = json.loads((project_path / "notebook.ipynb").read_text())
            assert raw_saved["metadata"] == notebook["metadata"]
            assert raw_saved["cells"][0] == notebook["cells"][0]
            marker = bridge.rpc("sessions.get_variable", project_id=project, session_id=session_id, name="secret_from_master")
            assert marker["value"] == 123456789
            edited = bridge.rpc("notebooks.apply_edits", project_id=project, notebook_id=notebook_id,
                                expected_revision=snapshot["revision"], edits=[{
                                    "op": "set_cell_metadata", "cell_id": "next", "metadata": {"closed_edit": True}}])
            assert edited["state"] == "DIRTY"
            closed = bridge.rpc("notebooks.close", project_id=project, notebook_id=notebook_id,
                                expected_revision=edited["document"]["revision"])
            assert closed["state"] == "CLEAN"
            assert json.loads((project_path / "notebook.ipynb").read_text())["cells"][3]["metadata"]["closed_edit"]
            opened = bridge.rpc("notebooks.open", project_id=project, relative_path="notebook.ipynb")
            assert opened["document"]["notebook_id"] == notebook_id
            assert bridge.rpc("sessions.ensure_primary", project_id=project, notebook_id=notebook_id)["session_id"] == session_id
            bridge.rpc("projects.remove_from_registry", expected_error="CK-PROJ-IN-USE", project_id=project)
            wait_package(bridge, project, bridge.rpc("packages.remove", project_id=project, packages=["cokernel-fixture-package"]))
            deadline = time.monotonic() + 5
            while bridge.rpc("sessions.get", project_id=project, session_id=session_id)["state"] != "STALE_ENVIRONMENT":
                assert time.monotonic() < deadline
                time.sleep(0.02)
            assert bridge.rpc("sessions.get_variable", project_id=project, session_id=session_id, name="secret_from_master")["value"] == 123456789

            large = {"nbformat": 4, "nbformat_minor": 5, "metadata": {"large": "z" * (9 * 1024 * 1024)}, "cells": []}
            large_id = import_notebook(bridge, project, "large.ipynb", large)["document"]["notebook_id"]
            bridge.rpc("notebooks.get", expected_error="CK-NB-LIMIT", project_id=project, notebook_id=large_id)
            chunk = bridge.rpc("notebooks.read_chunk", project_id=project, notebook_id=large_id, expected_revision=1, offset=0, length=262144)
            assert len(base64.b64decode(chunk["data_base64"], validate=True)) == 262144
            assert chunk["total_bytes"] > FRAME_LIMIT
            bridge.rpc("notebooks.read_chunk", expected_error="CK-NB-CONFLICT", project_id=project, notebook_id=large_id, expected_revision=0, offset=0, length=1)
            assert bridge.rpc("bridge.ping")["pong"]
            # An explicit stop may stop workers, but must retain unsaved document
            # memory and the RPC connection when final persistence conflicts.
            current = bridge.rpc("notebooks.get", project_id=project, notebook_id=notebook_id)
            bridge.rpc("notebooks.apply_edits", project_id=project, notebook_id=notebook_id,
                       expected_revision=current["revision"], edits=[{
                           "op": "set_notebook_metadata", "metadata": {"unsaved_shutdown": True}}])
            disk_path = project_path / "notebook.ipynb"
            outside = json.loads(disk_path.read_text())
            outside["metadata"]["external_writer"] = True
            disk_path.write_text(json.dumps(outside))
            bridge.rpc("bridge.shutdown", expected_error="CK-RUN-SHUTDOWN")
            assert bridge.rpc("bridge.ping")["pong"]
            retained = bridge.rpc("notebooks.get", project_id=project, notebook_id=notebook_id)
            assert retained["notebook"]["metadata"]["unsaved_shutdown"]
            assert json.loads(disk_path.read_text())["metadata"]["external_writer"]
            # Explicit reconciliation discards the known unsaved edit.
            bridge.rpc("notebooks.reload", project_id=project, notebook_id=notebook_id,
                       expected_revision=retained["revision"])
            deadline = time.monotonic() + 8
            while bridge.rpc("sessions.get", project_id=project, session_id=session_id)["state"] != "STOPPED":
                assert time.monotonic() < deadline
                time.sleep(0.02)
            assert bridge.rpc("bridge.shutdown")["stopped"]
            bridge.process.stdin.close()
            assert bridge.process.wait(timeout=5) == 0
            print(f"[notebook-runtime-smoke] PASS (nbformat {nbformat.__version__}; framed RPC, uv Project, real worker, persistence, close/reopen, stale environment, large-document transport)")
        finally:
            bridge.finish()
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
