"""Actual uv setup, cancellation/recovery and unrelated Session responsiveness."""
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile
import time

from smoke_notebook_runtime import Bridge, ROOT, import_notebook, wait_operation, wait_package
from smoke_async_packages import event


def wait_started(path):
    deadline = time.monotonic() + 5
    while not path.exists():
        assert time.monotonic() < deadline, "uv did not reach the test barrier"
        time.sleep(.01)
    return int(path.read_text())


def main():
    subprocess.run(["cargo", "build", "-q", "-p", "cokernel-runtime"], cwd=ROOT, check=True)
    with tempfile.TemporaryDirectory(prefix="cksetup-") as temporary:
        directory = Path(temporary)
        hold, started = directory / "hold", directory / "started"
        tools = directory / "bin"
        tools.mkdir()
        real_uv = shutil.which("uv")
        assert real_uv
        wrapper = tools / "uv"
        wrapper.write_text(f"#!{sys.executable}\nimport os,sys,time\nfrom pathlib import Path\n" +
            f"hold=Path({str(hold)!r})\nstarted=Path({str(started)!r})\n" +
            "if hold.exists() and sys.argv[1] in ('init','sync') and hold.read_text() in sys.argv:\n"
            " started.write_text(str(os.getpid()))\n deadline=time.monotonic()+30\n"
            " while hold.exists():\n"
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
            hello = bridge.rpc("bridge.hello", host_product_version="setup-proof", supported_protocols=[1])
            assert "async-projects" in hello["capabilities"]
            op = bridge.rpc("projects.create", name="active")
            project = wait_package(bridge, op["project_id"], op)["project"]["project_id"]
            notebook = {"nbformat": 4, "nbformat_minor": 5, "metadata": {}, "cells": [
                {"cell_type": "code", "id": "a", "source": "marker=123456789\nprint('ready', flush=True)\nimport time\ntime.sleep(60)", "metadata": {}, "execution_count": None, "outputs": []}
            ]}
            document = import_notebook(bridge, project, "proof.ipynb", notebook)["document"]
            session = bridge.rpc("sessions.ensure_primary", project_id=project, notebook_id=document["notebook_id"])["session_id"]
            execution = bridge.rpc("sessions.execute_cell", project_id=project, notebook_id=document["notebook_id"], session_id=session, cell_id="a", expected_revision=document["revision"])
            event(bridge, lambda e: e["event"] == "session.output" and e["payload"].get("session_id") == session)

            target = directory / "projects/new"
            hold.write_text(str(target))
            begin = time.monotonic()
            creating = bridge.rpc("projects.create", name="new")
            assert time.monotonic() - begin < 2
            pid = wait_started(started)
            assert bridge.rpc("bridge.ping")["pong"]
            bridge.rpc("projects.create", name="new", expected_error="CK-PROJ-EXISTS")
            (target / "user.txt").write_text("preserve user content")
            assert bridge.rpc("operations.cancel", project_id=creating["project_id"], operation_id=creating["operation_id"])["accepted"]
            cancelled = event(bridge, lambda e: e["event"] == "environment.operation_progress" and e["payload"].get("operation_id") == creating["operation_id"] and e["payload"]["result"]["state"] != "RUNNING")
            assert cancelled["payload"]["result"]["error"]["code"] == "CK-UV-CANCELLED"
            assert not Path(f"/proc/{pid}").exists()
            assert bridge.rpc("projects.get", project_id=creating["project_id"])["initialization_required"]
            assert bridge.rpc("sessions.get", project_id=project, session_id=session)["state"] == "EXECUTING"
            hold.unlink(); started.unlink()
            repaired = wait_package(bridge, creating["project_id"], bridge.rpc("packages.sync", project_id=creating["project_id"]))
            assert not repaired["initialization_required"] and not repaired["environment_reconcile_required"]
            assert (target / "user.txt").read_text() == "preserve user content"
            assert (target / ".venv/bin/python").is_file()

            imported = directory / "projects/registered"
            imported.mkdir()
            (imported / "pyproject.toml").write_text('[project]\nname="registered"\nversion="0.1.0"\nrequires-python=">=3.12"\ndependencies=[]\n')
            hold.write_text(str(imported))
            begin = time.monotonic()
            registering = bridge.rpc("projects.register", path=str(imported))
            assert time.monotonic() - begin < 2
            wait_started(started)
            bridge.rpc("projects.register", path=str(imported), expected_error="CK-UV-BUSY")
            begin = time.monotonic()
            bridge.rpc("sessions.interrupt", project_id=project, session_id=session)
            assert time.monotonic() - begin < 2
            assert wait_operation(bridge, project, execution["operation_id"])["execution_status"] == "INTERRUPTED"
            assert bridge.rpc("sessions.get_variable", project_id=project, session_id=session, name="marker")["value"] == 123456789
            hold.unlink(); started.unlink()
            terminal = event(bridge, lambda e: e["event"] == "environment.operation_progress" and e["payload"].get("operation_id") == registering["operation_id"] and e["payload"]["result"]["state"] != "RUNNING")
            assert terminal["payload"]["result"]["state"] == "SUCCEEDED", terminal
            again = bridge.rpc("projects.register", path=str(imported))
            assert again["project_id"] == registering["project_id"]
            wait_package(bridge, again["project_id"], again)

            hold.write_text(str(directory / "projects/unfinished"))
            unfinished = bridge.rpc("projects.create", name="unfinished")
            pid = wait_started(started)
            bridge.process.stdin.close()
            bridge.process.wait(timeout=10)
            assert bridge.process.returncode == 0 and not Path(f"/proc/{pid}").exists()
            hold.unlink()
        finally:
            hold.unlink(missing_ok=True)
            bridge.finish()
        reopened_logs = directory / "reopened"
        reopened_logs.mkdir()
        bridge = Bridge(environment, reopened_logs)
        try:
            incomplete = bridge.rpc("projects.get", project_id=unfinished["project_id"])
            assert incomplete["initialization_required"] and incomplete["environment_reconcile_required"]
            repaired = wait_package(bridge, unfinished["project_id"], bridge.rpc("packages.sync", project_id=unfinished["project_id"]))
            assert not repaired["initialization_required"] and not repaired["environment_reconcile_required"]
            bridge.rpc("bridge.shutdown")
            bridge.process.wait(timeout=10)
            assert bridge.process.returncode == 0
        finally:
            bridge.finish()
        print("[async-projects-smoke] PASS: actual uv create/register; responsive unrelated Session; cancellation preserves files; sync resumes initialization; stable identity on re-register; EOF/restart retains incomplete setup and repairs it")


if __name__ == "__main__":
    main()
