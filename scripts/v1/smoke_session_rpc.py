"""Actual worker proof for multiplexed startup/inspection and responsive control."""
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


def wait_pid(path):
    deadline = time.monotonic() + 5
    while time.monotonic() < deadline:
        try:
            return int(path.read_text())
        except (OSError, ValueError):
            time.sleep(.01)
    raise TimeoutError("worker launcher did not reach barrier")


def gone(pid):
    deadline = time.monotonic() + 5
    while Path(f"/proc/{pid}").exists():
        assert time.monotonic() < deadline, f"owned process {pid} survived cleanup"
        time.sleep(.01)


def main():
    subprocess.run(["cargo", "build", "-q", "-p", "cokernel-runtime"], cwd=ROOT, check=True)
    with tempfile.TemporaryDirectory(prefix="ckrpc-par-") as temporary:
        directory = Path(temporary)
        tools = directory / "bin"; tools.mkdir()
        release, pid_file, launches = directory / "release", directory / "pid", directory / "launches"
        real_uv = shutil.which("uv"); assert real_uv
        wrapper = tools / "uv"
        wrapper.write_text(f"#!{sys.executable}\nimport os,sys,time\nfrom pathlib import Path\n" +
            f"release=Path({str(release)!r})\npid=Path({str(pid_file)!r})\nlaunches=Path({str(launches)!r})\n" +
            "if sys.argv[1]=='run' and '--socket' in sys.argv and Path(sys.argv[sys.argv.index('--project')+1]).name=='blocked':\n"
            " with launches.open('a') as f: f.write(str(os.getpid())+'\\n')\n"
            " pid.write_text(str(os.getpid()))\n"
            " while not release.exists(): time.sleep(.01)\n" +
            f"os.execv({real_uv!r}, [{real_uv!r}, *sys.argv[1:]])\n")
        wrapper.chmod(0o700)
        environment = dict(os.environ)
        environment.update({"PATH": str(tools) + ":" + os.environ["PATH"],
            "COKERNEL_PROJECTS_ROOT": str(directory / "projects"),
            "COKERNEL_PROJECT_REGISTRY": str(directory / "state/projects.json"),
            "COKERNEL_WORKER_PACKAGE": str(ROOT / "worker"),
            "COKERNEL_SESSION_SOCKET_DIR": str(directory / "s"),
            "COKERNEL_FAILURE_DIR": str(directory / "failures")})
        bridge = Bridge(environment, directory)
        try:
            assert "concurrent-session-rpc" in bridge.rpc("bridge.hello", host_product_version="parallel-proof", supported_protocols=[1])["capabilities"]
            identities = []
            for name in ("blocked", "healthy"):
                created = bridge.rpc("projects.create", name=name)
                project = wait_package(bridge, created["project_id"], created)["project"]["project_id"]
                notebook = {"nbformat": 4, "nbformat_minor": 5, "metadata": {}, "cells": [{"cell_type":"code", "id":"a", "metadata":{}, "execution_count":None, "outputs":[], "source":"import os,sys,time,humanize\nworker_pid=os.getpid()\npython_base=os.path.realpath(sys.executable)\npackage_origin=os.path.realpath(humanize.__file__)\npackage_probe=humanize.intcomma(12345)\nmarker=123456789\nprint('ready', flush=True)\ntime.sleep(60)"}]}
                document = import_notebook(bridge, project, "proof.ipynb", notebook)["document"]
                identities.append((project, document["notebook_id"], document["revision"]))
            a, na, ra = identities[0]; b, nb, rb = identities[1]
            wait_package(bridge, b, bridge.rpc("packages.add", project_id=b, packages=["humanize==4.10.0"]))
            project_root = directory / "projects" / "healthy"
            dependency_truth = {name: (project_root / name).read_bytes() for name in ("pyproject.toml", "uv.lock")}
            first = bridge.send("sessions.ensure_primary", project_id=a, notebook_id=na)
            wait_pid(pid_file)
            second = bridge.send("sessions.ensure_primary", project_id=a, notebook_id=na)
            bridge.rpc("packages.sync", project_id=a, expected_error="CK-SESS-STARTING")
            bridge.rpc("projects.remove_from_registry", project_id=a, expected_error="CK-SESS-STARTING")
            bridge.rpc("sessions.restart", project_id=a, notebook_id=na, expected_error="CK-SESS-STARTING")
            healthy = bridge.receive(bridge.send("sessions.ensure_primary", project_id=b, notebook_id=nb), timeout=5)
            sid = healthy["session_id"]
            execution = bridge.rpc("sessions.execute_cell", project_id=b, notebook_id=nb, session_id=sid, cell_id="a", expected_revision=rb)
            event(bridge, lambda e: e["event"] == "session.output" and e["payload"].get("session_id") == sid)
            inspections = [bridge.send("sessions.get_variable", project_id=b, session_id=sid, name="marker") for _ in range(14)]
            bridge.rpc("sessions.get_variable", project_id=b, session_id=sid, name="marker", expected_error="CK-RUN-BUSY")
            begin = time.monotonic()
            assert bridge.rpc("bridge.ping")["pong"]
            bridge.rpc("sessions.interrupt", project_id=b, session_id=sid)
            assert time.monotonic() - begin < 2, "control waited for pending inspections/startup"
            for request in inspections:
                assert bridge.receive(request, timeout=5)["value"] == 123456789
            finished = wait_operation(bridge, b, execution["operation_id"])
            assert finished["execution_status"] == "INTERRUPTED"
            healthy_pid = bridge.rpc("sessions.get_variable", project_id=b, session_id=sid, name="worker_pid")["value"]
            release.touch()
            a1 = bridge.receive(first); a2 = bridge.receive(second)
            assert a1["session_id"] == a2["session_id"] and a1["worker_generation"] == a2["worker_generation"] == 1
            assert len(launches.read_text().splitlines()) == 1, "duplicate primary worker launch"
            assert bridge.rpc("runtime.get_snapshot")["session_requests"]["pending_requests"] == 0

            # Same Project, different notebook: same Project packages/base Python, separate
            # live namespace/process, and concurrent execution through the endpoint.
            other_source = "import os,sys,humanize\nworker_pid=os.getpid()\npython_base=os.path.realpath(sys.executable)\npackage_origin=os.path.realpath(humanize.__file__)\npackage_probe=humanize.intcomma(12345)\nother_marker=987"
            other_document = import_notebook(bridge, b, "other.ipynb", {"nbformat":4,"nbformat_minor":5,"metadata":{},"cells":[{"cell_type":"code","id":"other","metadata":{},"execution_count":None,"outputs":[],"source":other_source}]})["document"]
            other = bridge.rpc("sessions.ensure_primary", project_id=b, notebook_id=other_document["notebook_id"])
            assert other["session_id"] != sid
            again = bridge.rpc("sessions.execute_cell", project_id=b, notebook_id=nb, session_id=sid, cell_id="a", expected_revision=finished["revision"])
            event(bridge, lambda e: e["event"] == "session.output" and e["payload"].get("session_id") == sid)
            parallel = bridge.rpc("sessions.execute_cell", project_id=b, notebook_id=other_document["notebook_id"], session_id=other["session_id"], cell_id="other", expected_revision=other_document["revision"])
            assert wait_operation(bridge, b, parallel["operation_id"])["execution_status"] == "SUCCEEDED"
            assert bridge.rpc("operations.get", project_id=b, operation_id=again["operation_id"])["result"]["state"] == "PENDING"
            bridge.rpc("sessions.interrupt", project_id=b, session_id=sid)
            assert wait_operation(bridge, b, again["operation_id"])["execution_status"] == "INTERRUPTED"
            other_pid = bridge.rpc("sessions.get_variable", project_id=b, session_id=other["session_id"], name="worker_pid")["value"]
            assert other_pid != healthy_pid
            for name in ("python_base", "package_origin", "package_probe"):
                left = bridge.rpc("sessions.get_variable", project_id=b, session_id=sid, name=name)["value"]
                right = bridge.rpc("sessions.get_variable", project_id=b, session_id=other["session_id"], name=name)["value"]
                assert left == right, (name, left, right)
                if name == "package_origin":
                    assert Path(left).is_relative_to(project_root / ".venv"), left
                if name == "package_probe":
                    assert left == "12,345"
            bridge.rpc("sessions.get_variable", project_id=b, session_id=other["session_id"], name="marker", expected_error="CK-SESS-REQUEST")
            bridge.rpc("sessions.get_variable", project_id=b, session_id=sid, name="other_marker", expected_error="CK-SESS-REQUEST")

            release.unlink(); pid_file.unlink()
            restarting = bridge.send("sessions.restart", project_id=a, notebook_id=na)
            restart_pid = wait_pid(pid_file)
            bridge.rpc("sessions.execute_cell", project_id=a, notebook_id=na, session_id=a1["session_id"], cell_id="a", expected_revision=ra, expected_error="CK-SESS-STARTING")
            assert bridge.rpc("sessions.get_variable", project_id=b, session_id=sid, name="marker")["value"] == 123456789
            bridge.rpc("sessions.stop", project_id=a, session_id=a1["session_id"])
            bridge.receive(restarting, expected_error="CK-SESS-REQUEST-CANCELLED")
            gone(restart_pid)
            assert bridge.rpc("sessions.get", project_id=a, session_id=a1["session_id"])["state"] == "STOPPED"
            pid_file.unlink()
            bridge.send("sessions.ensure_primary", project_id=a, notebook_id=na)
            orphan = wait_pid(pid_file)
            bridge.process.stdin.close()
            bridge.process.wait(timeout=10)
            assert bridge.process.returncode == 0
            gone(orphan); gone(healthy_pid); gone(other_pid)
            assert dependency_truth == {name: (project_root / name).read_bytes() for name in dependency_truth}
        finally:
            release.touch()
            bridge.finish()
        print("[session-rpc-smoke] PASS: actual uv/IPython; shared startup; independent notebook startup; bounded pending inspections; responsive interrupt/status; correlated out-of-order replies; same-Project notebooks share environment but isolate namespaces and execute concurrently; restart/stop race; EOF reaps starting and active workers")


if __name__ == "__main__":
    main()
