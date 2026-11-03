"""Real Linux binaries: authenticated MCP joins an existing Runtime and can fail alone."""
import json
import os
from pathlib import Path
import selectors
import subprocess
import tempfile
import time
import urllib.error
import urllib.request

from smoke_notebook_runtime import Bridge, ROOT, import_notebook, wait_operation, wait_package


def start_mcp(directory, socket):
    stderr = (directory / "mcp.stderr").open("ab")
    child = subprocess.Popen(
        [str(ROOT / "target/debug/cokernel-mcp"), "serve", str(socket),
         str(directory / "bearer"), "0"],
        stdin=subprocess.DEVNULL, stdout=subprocess.PIPE, stderr=stderr, cwd=ROOT,
    )
    try:
        with selectors.DefaultSelector() as selector:
            selector.register(child.stdout, selectors.EVENT_READ)
            assert selector.select(10), "MCP readiness timed out"
        ready = json.loads(child.stdout.readline(4096))
        assert ready["event"] == "mcp.ready" and ready["authenticated"]
        return child, stderr, "http://" + ready["address"] + ready["path"]
    except BaseException:
        child.kill()
        child.wait(timeout=5)
        child.stdout.close()
        stderr.close()
        raise


def stop_mcp(child, stderr, crash=False):
    if child.poll() is None:
        if crash:
            child.kill()
        else:
            child.terminate()
        try:
            child.wait(timeout=5)
        except subprocess.TimeoutExpired:
            child.kill()
            child.wait(timeout=5)
            raise
    child.stdout.close()
    stderr.close()


def exercise_tools(rpc, bridge, project, document, session, worker):
    def tool(tool_name, error=None, **arguments):
        result=rpc("tools/call", {"name":tool_name,"arguments":arguments})
        if error:
            assert result["isError"] is True, tool_name
            assert error in result["content"][0]["text"], result
            return result
        assert result["isError"] is False, result
        return json.loads(result["content"][0]["text"])

    # Authoring from MCP only: no Desktop-created Project/document/cell prerequisite.
    created=tool("create_project",name="ai-authored")
    ai_project=created["project_id"]
    for _ in range(1200):
        receipt=tool("get_operation",project_id=ai_project,operation_id=created["operation_id"])
        if receipt["result"]["state"] in ("SUCCEEDED","FAILED"): break
        time.sleep(0.05)
    assert receipt["result"]["state"]=="SUCCEEDED",receipt
    doc=tool("create_notebook",project_id=ai_project,relative_path="ai.ipynb")["document"]
    ai={"project_id":ai_project,"notebook_id":doc["notebook_id"]}
    edited=tool("insert_code_cell",**ai,expected_revision=doc["revision"],index=0,cell_id="ai-cell",source="ai_value = 321\nai_value + 1")
    revision=edited["document"]["revision"]
    tool("set_cell_source",**ai,expected_revision=revision-1,cell_id="ai-cell",source="raise Exception()",error="CK-NB-CONFLICT")
    tool("save_notebook",**ai,expected_revision=revision)
    live_ai=tool("ensure_session",**ai)
    op=tool("execute_cell",**ai,session_id=live_ai["session_id"],expected_revision=revision,cell_id="ai-cell")
    assert wait_operation(bridge,ai_project,op["operation_id"])["execution_status"]=="SUCCEEDED"
    assert tool("get_variable",project_id=ai_project,session_id=live_ai["session_id"],name="ai_value")["value"]==321
    assert tool("open_notebook",project_id=ai_project,relative_path="ai.ipynb")["document"]["notebook_id"]==doc["notebook_id"]
    tool("create_notebook",project_id=ai_project,relative_path="../escape.ipynb",error="CK-NB-")
    tool("add_packages",project_id=ai_project,packages=["--help"],error="CK-MCP-PARAMS")
    tool("stop_session",project_id=ai_project,session_id=live_ai["session_id"])

    notebook=document["notebook_id"]
    identity={"project_id":project,"notebook_id":notebook}
    live={"project_id":project,"session_id":session}
    assert tool("ensure_session",**identity)["session_id"]==session
    assert tool("get_session",**live)["notebook_id"]==notebook
    assert any(s["session_id"]==session for s in tool("list_sessions",project_id=project))
    listing=tool("list_notebooks",project_id=project)
    assert any(s["document"]["notebook_id"]==notebook for s in listing["open_documents"])
    assert tool("get_variable",name="secret_from_master",**live)["value"]==123456789
    assert tool("get_variable",name="worker_pid",**live)["value"]==worker
    assert tool("get_variable",name="working_directory",**live)["value"]==bridge.rpc("projects.get",project_id=project)["project"]["root_path"]
    assert any(v["name"]=="secret_from_master" for v in tool("list_variables",**live))
    assert "stale" in tool("get_resources")
    page=tool("read_notebook",**identity)
    assert page["cells"][0]["cell_id"]=="state"
    revision=page["revision"]
    tool("read_notebook",error="CK-NB-CONFLICT",expected_revision=revision-1,**identity)
    tool("execute_cell",error="CK-NB-CONFLICT",cell_id="state",expected_revision=revision-1,session_id=session,**identity)
    for expression in ["secret_from_master + 1","__import__('os')","worker_pid.__class__","class"]:
        tool("get_variable",error="CK-MCP-PARAMS",name=expression,**live)
    tool("execute_cell",error="CK-MCP-PARAMS",cell_id="state",expected_revision=revision,session_id=session,code="raise Exception()",**identity)
    other=bridge.rpc("projects.create",name="other")
    foreign=wait_package(bridge,other["project_id"],other)["project"]["project_id"]
    tool("get_variable",error="CK-NB-NOT-FOUND",project_id=foreign,session_id=session,name="secret_from_master")
    tool("read_notebook",error="CK-NB-NOT-FOUND",project_id=foreign,notebook_id=notebook)
    tool("ensure_session",error="CK-NB-NOT-FOUND",project_id=foreign,notebook_id=notebook)
    tool("execute_cell",error="CK-NB-NOT-FOUND",project_id=foreign,notebook_id=notebook,session_id=session,cell_id="state",expected_revision=revision)
    huge={"nbformat":4,"nbformat_minor":5,"metadata":{},"cells":[{"cell_type":"code","id":"huge","source":"変数\n"*40_000,"metadata":{},"outputs":[{"output_type":"display_data","data":{"image/png":"x"*500_000},"metadata":{}}],"execution_count":1}]}
    huge_doc=import_notebook(bridge,project,"huge.ipynb",huge)["document"]
    huge_page=tool("read_notebook",project_id=project,notebook_id=huge_doc["notebook_id"],include_outputs=True)
    assert huge_page["cells"][0]["source_truncated"] and huge_page["cells"][0]["outputs_truncated"]
    assert len(json.dumps(huge_page).encode())<128*1024

    # Human A is accepted first; MCP B must see A's completed state.
    release=Path(bridge.rpc("projects.get",project_id=project)["project"]["root_path"])/"mcp_fifo_release"
    assert not release.exists()
    source_a="import time\nfrom pathlib import Path\norder=['human-start']\nwhile not Path('mcp_fifo_release').exists(): time.sleep(.01)\nsecret_from_master+=1\norder.append('human-end')"
    source_b="assert order==['human-start','human-end']\norder.append('mcp')\nsecret_from_master+=1"
    hostile="""hostile_touched=0
class Trap:
 def touch(self,*args):
  global hostile_touched
  hostile_touched+=1
  raise RuntimeError('inspection executed user code')
 __repr__=__str__=__iter__=__getattr__=__add__=touch
class TrapList(list):
 __repr__=__str__=__iter__=__getattr__=__add__=Trap.touch
hostile=Trap()
hostile_list=TrapList([1])
escaped='\\x01'*65536
"""
    edits=[{"op":"insert_cell","index":i+1,"cell":{"cell_type":"code","id":name,"source":source,"metadata":{},"outputs":[],"execution_count":None}} for i,(name,source) in enumerate([("human",source_a),("ai",source_b),("hostile",hostile),("long","import time\ntime.sleep(60)")])]
    edited=bridge.rpc("notebooks.apply_edits",expected_revision=revision,edits=edits,**identity)
    revision=edited["document"]["revision"]
    human=bridge.rpc("sessions.execute_cell",cell_id="human",expected_revision=revision,session_id=session,**identity)
    ai=tool("execute_cell",cell_id="ai",expected_revision=revision,session_id=session,**identity)
    assert ai["session_id"]==session
    assert bridge.rpc("operations.get",project_id=project,operation_id=human["operation_id"])["result"]["state"]=="PENDING"
    release.write_text("release",encoding="utf-8")
    assert wait_operation(bridge,project,human["operation_id"])["execution_status"]=="SUCCEEDED"
    def finished(operation):
        deadline=time.monotonic()+20
        while time.monotonic()<deadline:
            status=tool("get_operation",project_id=project,operation_id=operation)["result"]
            if status["state"]!="PENDING":
                return status
            time.sleep(.02)
        raise TimeoutError("MCP operation did not finish")
    assert finished(ai["operation_id"])["execution_status"]=="SUCCEEDED"
    assert tool("get_variable",name="order",**live)["value"]==["human-start","human-end","mcp"]
    assert tool("get_variable",name="secret_from_master",**live)["value"]==123456791
    tool("get_operation",error="CK-NB-NOT-FOUND",project_id=foreign,operation_id=ai["operation_id"])
    revision=tool("read_notebook",**identity)["revision"]
    op=tool("execute_cell",cell_id="hostile",expected_revision=revision,session_id=session,**identity)
    assert finished(op["operation_id"])["execution_status"]=="SUCCEEDED"
    for name in ["hostile","hostile_list"]:
        assert tool("get_variable",name=name,**live)["supported"] is False
    assert tool("get_variable",name="hostile_touched",**live)["value"]==0
    tool("get_variable",error="CK-MCP-LIMIT",name="escaped",**live)
    parallel_doc=import_notebook(bridge,project,"parallel.ipynb",{"nbformat":4,"nbformat_minor":5,"metadata":{},"cells":[{"cell_type":"code","id":"parallel","source":"import os\nassert 'secret_from_master' not in globals()\nisolated=987\ndifferent_pid=os.getpid()","metadata":{},"outputs":[],"execution_count":None}]})["document"]
    parallel=tool("ensure_session",project_id=project,notebook_id=parallel_doc["notebook_id"])["session_id"]
    assert parallel!=session
    revision=tool("read_notebook",**identity)["revision"]
    op=tool("execute_cell",cell_id="long",expected_revision=revision,session_id=session,**identity)
    deadline=time.monotonic()+5
    while tool("get_session",**live)["state"]!="EXECUTING":
        assert time.monotonic()<deadline;time.sleep(.01)
    other_op=tool("execute_cell",project_id=project,notebook_id=parallel_doc["notebook_id"],session_id=parallel,cell_id="parallel",expected_revision=parallel_doc["revision"])
    assert finished(other_op["operation_id"])["execution_status"]=="SUCCEEDED"
    assert tool("get_session",**live)["state"]=="EXECUTING"
    assert tool("get_variable",project_id=project,session_id=parallel,name="different_pid")["value"]!=worker
    assert tool("get_variable",project_id=project,session_id=parallel,name="isolated")["value"]==987
    assert tool("interrupt_session",**live)["accepted"]
    assert finished(op["operation_id"])["execution_status"]=="INTERRUPTED"
    tool("get_variable",error="CK-SESS-REQUEST",name="isolated",**live)
    restarted=tool("restart_session",**identity)
    session=restarted["session_id"];live["session_id"]=session
    tool("get_variable",error="CK-SESS-REQUEST",name="secret_from_master",**live)
    assert tool("stop_session",**live)["accepted"]
    deadline=time.monotonic()+5
    while tool("get_session",**live)["state"]!="STOPPED":
        assert time.monotonic()<deadline;time.sleep(.01)
    session=tool("ensure_session",**identity)["session_id"];live["session_id"]=session
    revision=tool("read_notebook",**identity)["revision"]
    op=tool("execute_cell",cell_id="state",expected_revision=revision,session_id=session,**identity)
    assert finished(op["operation_id"])["execution_status"]=="SUCCEEDED"
    return session, tool("get_variable",name="worker_pid",**live)["value"]


def main():
    subprocess.run(["cargo", "build", "-q", "-p", "cokernel-runtime", "-p", "cokernel-mcp"], cwd=ROOT, check=True)
    with tempfile.TemporaryDirectory(prefix="ckmcp-life-") as temporary:
        directory = Path(temporary)
        socket = directory / "runtime.sock"
        environment = dict(os.environ)
        environment.update({
            "COKERNEL_RUNTIME_SOCKET": str(socket),
            "COKERNEL_PROJECTS_ROOT": str(directory / "projects"),
            "COKERNEL_PROJECT_REGISTRY": str(directory / "state/projects.json"),
            "COKERNEL_WORKER_PACKAGE": str(ROOT / "worker"),
            "COKERNEL_SESSION_SOCKET_DIR": str(directory / "s"),
            "COKERNEL_FAILURE_DIR": str(directory / "failures"),
        })
        bridge = Bridge(environment, directory)
        child = stderr = None
        try:
            assert bridge.rpc("bridge.ping")["pong"]
            created = bridge.rpc("projects.create", name="shared")
            project = wait_package(bridge, created["project_id"], created)["project"]["project_id"]
            notebook = {"nbformat": 4, "nbformat_minor": 5, "metadata": {}, "cells": [{
                "cell_type": "code", "id": "state", "metadata": {}, "execution_count": None,
                "outputs": [], "source": "import os\nworker_pid=os.getpid()\nworking_directory=os.getcwd()\nsecret_from_master=123456789",
            }]}
            document = import_notebook(bridge, project, "shared.ipynb", notebook)["document"]
            session = bridge.rpc("sessions.ensure_primary", project_id=project, notebook_id=document["notebook_id"])["session_id"]
            execution = bridge.rpc("sessions.execute_cell", project_id=project, notebook_id=document["notebook_id"], session_id=session, cell_id="state", expected_revision=document["revision"])
            assert wait_operation(bridge, project, execution["operation_id"])["execution_status"] == "SUCCEEDED"
            worker = bridge.rpc("sessions.get_variable", project_id=project, session_id=session, name="worker_pid")["value"]
            child, stderr, url = start_mcp(directory, socket)
            bearer = (directory / "bearer").read_text()
            opener = urllib.request.build_opener(urllib.request.ProxyHandler({}))

            def rpc(method, params=None, authenticate=True):
                body = json.dumps({"jsonrpc": "2.0", "id": 1, "method": method, "params": params or {}}).encode()
                headers = {"Content-Type": "application/json", "Accept": "application/json, text/event-stream", "MCP-Protocol-Version": "2025-11-25"}
                if authenticate:
                    headers["Authorization"] = "Bearer " + bearer
                request = urllib.request.Request(url, data=body, headers=headers)
                with opener.open(request, timeout=5) as response:
                    data = response.read(256 * 1024 + 1)
                    assert len(data) <= 256 * 1024
                    assert bearer.encode() not in data
                    value = json.loads(data)
                    assert "error" not in value
                    return value["result"]

            try:
                rpc("tools/list", authenticate=False)
                raise AssertionError("MCP allowed unauthenticated access")
            except urllib.error.HTTPError as error:
                assert error.code == 401
                error.close()
            initialized = rpc("initialize", {"protocolVersion": "2025-11-25", "capabilities": {}, "clientInfo": {"name": "lifecycle-proof", "version": "1"}})
            assert initialized["protocolVersion"] == "2025-11-25"
            assert {tool["name"] for tool in rpc("tools/list")["tools"]} == {"list_projects", "list_notebooks", "read_notebook", "list_sessions", "get_session", "ensure_session", "execute_cell", "interrupt_session", "restart_session", "stop_session", "list_variables", "get_variable", "get_runtime_status", "get_resources", "get_operation", "create_project", "create_notebook", "open_notebook", "insert_code_cell", "set_cell_source", "add_packages", "save_notebook"}
            for tool in ["list_projects", "get_runtime_status"]:
                result = rpc("tools/call", {"name": tool, "arguments": {}})
                assert result["isError"] is False
                content = json.loads(result["content"][0]["text"])
                if tool == "list_projects":
                    assert any(record["project"]["project_id"] == project for record in content)
                else:
                    assert "sessions" in content["capabilities"]
            current=bridge.rpc("notebooks.get",project_id=project,notebook_id=document["notebook_id"])
            edited=bridge.rpc("notebooks.apply_edits",project_id=project,notebook_id=document["notebook_id"],expected_revision=current["revision"],edits=[{"op":"insert_cell","index":1,"cell":{"cell_type":"code","id":"survive","source":"import time\ntime.sleep(1)\nsurvived_mcp_crash=True","metadata":{},"outputs":[],"execution_count":None}}])
            accepted=rpc("tools/call",{"name":"execute_cell","arguments":{"project_id":project,"notebook_id":document["notebook_id"],"session_id":session,"cell_id":"survive","expected_revision":edited["document"]["revision"]}})
            assert not accepted["isError"]
            accepted=json.loads(accepted["content"][0]["text"])
            deadline=time.monotonic()+5
            while bridge.rpc("sessions.get",project_id=project,session_id=session)["state"]!="EXECUTING":
                assert time.monotonic()<deadline;time.sleep(.01)
            stop_mcp(child, stderr, crash=True)
            child = stderr = None
            assert bridge.process.poll() is None
            assert wait_operation(bridge,project,accepted["operation_id"])["execution_status"]=="SUCCEEDED"
            assert bridge.rpc("sessions.get_variable",project_id=project,session_id=session,name="survived_mcp_crash")["value"] is True
            assert bridge.rpc("sessions.get_variable", project_id=project, session_id=session, name="secret_from_master")["value"] == 123456789
            assert bridge.rpc("sessions.get_variable", project_id=project, session_id=session, name="worker_pid")["value"] == worker
            child, stderr, url = start_mcp(directory, socket)
            assert (directory / "bearer").read_text() == bearer
            assert not rpc("tools/call", {"name": "get_runtime_status", "arguments": {}})["isError"]
            session, worker = exercise_tools(rpc,bridge,project,document,session,worker)
            # Product credentials are not inherited; development same-UID file
            # isolation is deliberately not claimed by this proof.
            for proc in [bridge.process.pid, worker, child.pid]:
                for field in ["environ", "cmdline"]:
                    assert bearer.encode() not in Path(f"/proc/{proc}/{field}").read_bytes()
            stop_mcp(child, stderr)
            assert child.returncode == 0
            child = stderr = None
            assert bridge.rpc("sessions.ensure_primary", project_id=project, notebook_id=document["notebook_id"])["session_id"] == session
            assert bridge.rpc("bridge.shutdown")["stopped"]
            assert bridge.process.wait(timeout=10) == 0
            assert not socket.exists()
            deadline = time.monotonic() + 5
            while Path(f"/proc/{worker}").exists():
                assert time.monotonic() < deadline, "worker survived Runtime shutdown"
                time.sleep(.01)
            for log in ["mcp.stderr", "runtime.stderr"]:
                assert bearer.encode() not in (directory / log).read_bytes()
        finally:
            if child is not None:
                stop_mcp(child, stderr, crash=True)
            bridge.finish()
    print("[mcp-lifecycle-smoke] PASS: 22 actual HTTP tools; Human value 123456789 read by MCP in same primary/worker; Human/MCP FIFO; authoritative cell/revision; Project ownership; hostile inspection; bounded responses; interrupt/restart/stop; MCP SIGKILL/restart preserves live worker; bearer absent from args/environment/logs; owned cleanup. Desktop/Codex/Web ChatGPT and managed isolation remain pending.")


if __name__ == "__main__":
    main()
