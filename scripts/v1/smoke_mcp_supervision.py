"""Actual Runtime-owned MCP recovery; no harness-created replacement process."""
import json
import os
from pathlib import Path
import signal
import tempfile
import time
import urllib.request

from smoke_notebook_runtime import Bridge, ROOT, import_notebook, wait_operation, wait_package


def environment(directory):
    result = dict(os.environ)
    result.update({
        "COKERNEL_RUNTIME_SOCKET": str(directory / "runtime.sock"),
        "COKERNEL_PROJECTS_ROOT": str(directory / "projects"),
        "COKERNEL_PROJECT_REGISTRY": str(directory / "state/projects.json"),
        "COKERNEL_WORKER_PACKAGE": str(ROOT / "worker"),
        "COKERNEL_SESSION_SOCKET_DIR": str(directory / "s"),
        "COKERNEL_FAILURE_DIR": str(directory / "failures"),
        "COKERNEL_MCP_EXECUTABLE": str(ROOT / "target/debug/cokernel-mcp"),
        "COKERNEL_MCP_BEARER_FILE": str(directory / "bearer"),
        "COKERNEL_MCP_PORT": "0",
        "COKERNEL_SUPERVISION_SECRET_SENTINEL": "must-not-reach-mcp-or-worker",
    })
    return result


def status_until(bridge, predicate, seconds=18):
    deadline = time.monotonic() + seconds
    while True:
        # Recovery/startup never monopolizes Runtime admission.
        status = bridge.receive(bridge.send("runtime.get_mcp_status"), timeout=2)
        assert bridge.receive(bridge.send("bridge.ping"), timeout=2)["pong"]
        if predicate(status):
            return status
        assert time.monotonic() < deadline, status
        time.sleep(.025)


def gone(pid):
    assert not Path(f"/proc/{pid}").exists(), f"child was not reaped: {pid}"


def main():
    with tempfile.TemporaryDirectory(prefix="ck-mcp-owned-") as tmp:
        directory = Path(tmp)
        bridge = Bridge(environment(directory), directory)
        try:
            ready = status_until(bridge, lambda s: s["state"] == "READY")
            address = ready["address"]
            bearer = (directory / "bearer").read_text()
            opener = urllib.request.build_opener(urllib.request.ProxyHandler({}))

            def tool(tool_name, **arguments):
                request = urllib.request.Request("http://" + address + "/mcp",
                    data=json.dumps({"jsonrpc": "2.0", "id": 1, "method": "tools/call", "params": {"name": tool_name, "arguments": arguments}}).encode(),
                    headers={"Authorization": "Bearer " + bearer, "Content-Type": "application/json", "Accept": "application/json, text/event-stream", "MCP-Protocol-Version": "2025-11-25"})
                with opener.open(request, timeout=5) as response:
                    data = response.read(256 * 1024 + 1)
                assert len(data) <= 256 * 1024 and bearer.encode() not in data
                result = json.loads(data)["result"]
                assert not result["isError"], result
                return json.loads(result["content"][0]["text"])

            assert tool("get_runtime_status")["mcp"]["state"] == "READY"
            created = bridge.rpc("projects.create", name="shared")
            project = wait_package(bridge, created["project_id"], created)["project"]["project_id"]
            notebook = {"nbformat": 4, "nbformat_minor": 5, "metadata": {}, "cells": [
                {"cell_type": "code", "id": "state", "metadata": {}, "execution_count": None, "outputs": [],
                 "source": "import os\nworker_pid=os.getpid()\nsecret_from_master=123456789"},
                {"cell_type": "code", "id": "survive", "metadata": {}, "execution_count": None, "outputs": [],
                 "source": "import time\ntime.sleep(1)\nsecret_from_master+=1"},
            ]}
            document = import_notebook(bridge, project, "shared.ipynb", notebook)["document"]
            identity = {"project_id": project, "notebook_id": document["notebook_id"]}
            session = bridge.rpc("sessions.ensure_primary", **identity)["session_id"]
            live = {"project_id": project, "session_id": session}

            def execute(cell):
                revision = bridge.rpc("notebooks.get", **identity)["revision"]
                return bridge.rpc("sessions.execute_cell", **identity, session_id=session, cell_id=cell, expected_revision=revision)

            accepted = execute("state")
            assert wait_operation(bridge, project, accepted["operation_id"])["execution_status"] == "SUCCEEDED"
            worker = bridge.rpc("sessions.get_variable", name="worker_pid", **live)["value"]
            assert tool("get_variable", name="secret_from_master", **live)["value"] == 123456789
            accepted = execute("survive")
            deadline = time.monotonic() + 5
            while bridge.rpc("sessions.get", **live)["state"] != "EXECUTING":
                assert time.monotonic() < deadline
                time.sleep(.01)
            os.kill(ready["pid"], signal.SIGKILL)
            recovered = status_until(bridge, lambda s: s["state"] == "READY" and s["generation"] == 2)
            gone(ready["pid"])
            assert recovered["address"] == address
            assert recovered["failures"][-1]["signal"] == signal.SIGKILL
            assert wait_operation(bridge, project, accepted["operation_id"])["execution_status"] == "SUCCEEDED"
            assert tool("get_variable", name="secret_from_master", **live)["value"] == 123456790
            assert tool("get_variable", name="worker_pid", **live)["value"] == worker
            assert tool("ensure_session", **identity)["session_id"] == session
            assert (directory / "bearer").read_text() == bearer

            # Three more rapid crashes consume the automatic retry allowance.
            for generation in [2, 3, 4]:
                os.kill(recovered["pid"], signal.SIGKILL)
                old = recovered["pid"]
                recovered = status_until(bridge, lambda s: s["state"] == ("FAILED" if generation == 4 else "READY") and s["generation"] == (4 if generation == 4 else generation + 1))
                gone(old)
            assert recovered["consecutive_failures"] == 4
            assert len(recovered["failures"]) == 4
            assert bridge.rpc("runtime.get_snapshot")["mcp"]["state"] == "FAILED"
            accepted = execute("survive")
            assert wait_operation(bridge, project, accepted["operation_id"])["execution_status"] == "SUCCEEDED"
            assert bridge.rpc("sessions.get_variable", name="secret_from_master", **live)["value"] == 123456791
            assert bridge.rpc("mcp.restart")["accepted"]
            recovered = status_until(bridge, lambda s: s["state"] == "READY" and s["generation"] == 5)
            assert recovered["address"] == address and recovered["consecutive_failures"] == 0
            assert tool("get_variable", name="secret_from_master", **live)["value"] == 123456791
            # Stopped-but-alive service is detected by authenticated-endpoint health.
            os.kill(recovered["pid"], signal.SIGSTOP)
            old = recovered["pid"]
            recovered = status_until(bridge, lambda s: s["state"] == "READY" and s["generation"] == 6)
            gone(old)
            assert recovered["failures"][-1]["code"] == "CK-MCP-HEALTH"
            assert tool("get_variable", name="worker_pid", **live)["value"] == worker
            for pid in [worker, recovered["pid"]]:
                for field in ["cmdline", "environ"]:
                    data = Path(f"/proc/{pid}/{field}").read_bytes()
                    assert bearer.encode() not in data
                    assert b"must-not-reach-mcp-or-worker" not in data
            assert bridge.rpc("bridge.shutdown")["stopped"]
            gone(recovered["pid"])
            assert bridge.process.wait(timeout=10) == 0
            gone(worker)
            assert bearer.encode() not in (directory / "runtime.stderr").read_bytes()
        finally:
            bridge.finish()

    # Invalid optional MCP configuration cannot take down local Runtime.
    for mode in ["config", "socket", "executable"]:
        with tempfile.TemporaryDirectory(prefix="ck-mcp-bad-") as tmp:
            directory = Path(tmp)
            env = environment(directory)
            expected = "CK-MCP-CONFIG"
            if mode == "config":
                env["COKERNEL_MCP_PORT"] = "invalid"
            elif mode == "socket":
                (directory / "runtime.sock").write_text("preserve me")
                expected = "CK-MCP-API"
            else:
                env["COKERNEL_MCP_EXECUTABLE"] = str(directory / "missing")
                expected = "CK-MCP-LAUNCH"
            bridge = Bridge(env, directory)
            try:
                failed = status_until(bridge, lambda s: s["state"] == "FAILED")
                assert failed["failures"][-1]["code"] == expected
                assert bridge.rpc("projects.list") == []
                if mode == "socket":
                    assert (directory / "runtime.sock").read_text() == "preserve me"
                assert bridge.rpc("bridge.shutdown")["stopped"]
                assert bridge.process.wait(timeout=10) == 0
            finally:
                bridge.finish()
    print("[mcp-supervision-smoke] PASS: actual Runtime-owned ready/crash/backoff/circuit/manual-restart/hung-health/shutdown; stable endpoint/bearer; live Notebook primary/PID/value and accepted execution survive; bad configuration is local-only degradation; no credential/sentinel inheritance. Managed identity and actual AI clients remain pending.")


if __name__ == "__main__":
    main()
