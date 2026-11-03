"""Real Runtime/uv/worker proof: control stays responsive during package work."""
import json
import os
from pathlib import Path
import shutil
import struct
import subprocess
import sys
import tempfile
import time

from smoke_notebook_runtime import Bridge, ROOT, import_notebook, wait_operation, wait_package


def event(bridge, predicate, seconds=20):
    deadline = time.monotonic() + seconds
    while time.monotonic() < deadline:
        item = bridge.events.get(timeout=max(0.01, deadline - time.monotonic()))
        if predicate(item):
            return item
    raise TimeoutError("expected domain event did not arrive")


def main():
    subprocess.run(["cargo", "build", "-q", "-p", "cokernel-runtime"], cwd=ROOT, check=True)
    with tempfile.TemporaryDirectory(prefix="ckasync-") as temporary:
        directory = Path(temporary)
        release = directory / "release"
        started = directory / "started"
        tools = directory / "bin"
        tools.mkdir()
        real_uv = shutil.which("uv")
        assert real_uv
        wrapper = tools / "uv"
        # Delay only dependency addition; the actual operation still uses real uv.
        wrapper.write_text(f"#!{sys.executable}\n" +
            "import os,sys,time\nfrom pathlib import Path\n" +
            f"release=Path({str(release)!r})\nstarted=Path({str(started)!r})\n" +
            "if sys.argv[1:2] == ['add']:\n"
            " started.write_text(str(os.getpid()))\n deadline=time.monotonic()+25\n"
            " while not release.exists():\n"
            "  if time.monotonic()>deadline: sys.exit(90)\n"
            "  time.sleep(.01)\n" +
            f"os.execv({real_uv!r}, [{real_uv!r}, *sys.argv[1:]])\n")
        wrapper.chmod(0o700)
        environment = dict(os.environ)
        environment.update({
            "PATH": str(tools) + ":" + os.environ["PATH"],
            "COKERNEL_PROJECTS_ROOT": str(directory / "projects"),
            "COKERNEL_PROJECT_REGISTRY": str(directory / "state/projects.json"),
            "COKERNEL_WORKER_PACKAGE": str(ROOT / "worker"),
            "COKERNEL_SESSION_SOCKET_DIR": str(directory / "s"),
            "COKERNEL_FAILURE_DIR": str(directory / "failures"),
        })
        bridge = Bridge(environment, directory)
        try:
            hello = bridge.rpc("bridge.hello", host_product_version="async-proof", supported_protocols=[1])
            assert "async-packages" in hello["capabilities"]
            created = bridge.rpc("projects.create", name="proof")
            project = wait_package(bridge, created["project_id"], created)["project"]["project_id"]
            notebook = {"nbformat": 4, "nbformat_minor": 5, "metadata": {}, "cells": [
                {"cell_type": "code", "id": "a", "source": "secret_from_master=123456789\nprint('ready', flush=True)\nimport time\ntime.sleep(20)", "metadata": {}, "execution_count": None, "outputs": []}
            ]}
            document = import_notebook(bridge, project, "proof.ipynb", notebook)["document"]
            notebook_id = document["notebook_id"]
            session = bridge.rpc("sessions.ensure_primary", project_id=project, notebook_id=notebook_id)
            session_id = session["session_id"]
            execution = bridge.rpc("sessions.execute_cell", project_id=project, notebook_id=notebook_id,
                                   session_id=session_id, cell_id="a", expected_revision=document["revision"])
            event(bridge, lambda e: e["event"] == "session.output" and e["payload"].get("session_id") == session_id)
            begin = time.monotonic()
            package = bridge.rpc("packages.add", project_id=project, packages=[str(ROOT / "tests/fixtures/python-package")])
            assert time.monotonic() - begin < 2, "uv blocked admission"
            deadline = time.monotonic() + 5
            while not started.exists():
                assert time.monotonic() < deadline
                time.sleep(.01)
            assert not release.exists()
            begin = time.monotonic()
            assert bridge.rpc("bridge.ping")["pong"]
            assert bridge.rpc("operations.get", project_id=project, operation_id=package["operation_id"])["result"]["state"] == "RUNNING"
            assert bridge.rpc("sessions.ensure_primary", project_id=project, notebook_id=notebook_id)["session_id"] == session_id
            bridge.rpc("sessions.restart", expected_error="CK-UV-BUSY", project_id=project, notebook_id=notebook_id)
            assert bridge.rpc("environment.get_status", project_id=project)["reconcile_required"]
            bridge.rpc("packages.sync", expected_error="CK-UV-BUSY", project_id=project)
            bridge.rpc("bridge.shutdown", expected_error="CK-UV-BUSY")
            bridge.rpc("sessions.interrupt", project_id=project, session_id=session_id)
            assert time.monotonic() - begin < 2, "control blocked behind uv"
            result = wait_operation(bridge, project, execution["operation_id"])
            assert result["state"] == "PERSISTED" and result["execution_status"] == "INTERRUPTED", result
            assert bridge.rpc("sessions.get_variable", project_id=project, session_id=session_id, name="secret_from_master")["value"] == 123456789

            # Interrupt a bridge read in the middle of its header while uv finishes.
            # Completion must be published without any new complete RPC request,
            # and resuming that same partial frame must still decode correctly.
            bridge.sequence += 1
            request_id = str(bridge.sequence)
            body = json.dumps({"protocol": 1, "request_id": request_id, "method": "bridge.ping", "params": {}}).encode()
            frame = struct.pack(">I", len(body)) + body
            bridge.process.stdin.write(frame[:2]); bridge.process.stdin.flush()
            release.touch()
            completed = event(bridge, lambda e: e["event"] == "environment.operation_progress" and e["payload"].get("operation_id") == package["operation_id"] and e["payload"]["result"]["state"] != "RUNNING")
            assert completed["payload"]["result"]["state"] == "SUCCEEDED", completed
            bridge.process.stdin.write(frame[2:]); bridge.process.stdin.flush()
            response = bridge.results.get(timeout=3)
            assert response["request_id"] == request_id and response["ok"], response
            status = bridge.rpc("sessions.get", project_id=project, session_id=session_id)
            assert status["state"] == "STALE_ENVIRONMENT" and status["session_id"] == session_id, status
            assert bridge.rpc("sessions.get_variable", project_id=project, session_id=session_id, name="secret_from_master")["value"] == 123456789
            # Explicit cancellation reaps uv and marks uncertainty without killing
            # the existing Session or treating cancellation as a Python interrupt.
            release.unlink(); started.unlink()
            cancelled = bridge.rpc("packages.add", project_id=project, packages=[str(ROOT / "tests/fixtures/python-package")])
            deadline = time.monotonic() + 5
            while not started.exists():
                assert time.monotonic() < deadline
                time.sleep(.01)
            cancelled_pid = int(started.read_text())
            assert bridge.rpc("operations.cancel", project_id=project, operation_id=cancelled["operation_id"])["accepted"]
            terminal = event(bridge, lambda e: e["event"] == "environment.operation_progress" and e["payload"].get("operation_id") == cancelled["operation_id"] and e["payload"]["result"]["state"] != "RUNNING")
            assert terminal["payload"]["result"]["error"]["code"] == "CK-UV-CANCELLED", terminal
            assert not Path(f"/proc/{cancelled_pid}").exists()
            assert bridge.rpc("sessions.get_variable", project_id=project, session_id=session_id, name="secret_from_master")["value"] == 123456789
            assert not bridge.rpc("operations.cancel", project_id=project, operation_id=cancelled["operation_id"])["accepted"]
            bridge.rpc("sessions.restart", expected_error="CK-UV-NEEDS-SYNC", project_id=project, notebook_id=notebook_id)
            started.unlink()
            bridge.rpc("packages.add", project_id=project, packages=[str(ROOT / "tests/fixtures/python-package")])
            deadline = time.monotonic() + 5
            while not started.exists():
                assert time.monotonic() < deadline
                time.sleep(.01)
            orphan_candidate = int(started.read_text())
            bridge.process.stdin.close()
            bridge.process.wait(timeout=10)
            assert bridge.process.returncode == 0
            assert not Path(f"/proc/{orphan_candidate}").exists()
            registry = json.loads((directory / "state/projects.json").read_text())
            assert registry["projects"][0]["project"]["environment_generation"] == 4, registry
            print("[async-packages-smoke] PASS: real uv; responsive interrupt/status; shared memory survives; stale generation; completion without polling; partial-frame read survives completion; explicit cancellation; EOF cancels/reaps uv and reconciles registry")
        finally:
            release.touch()
            bridge.finish()


if __name__ == "__main__":
    main()
