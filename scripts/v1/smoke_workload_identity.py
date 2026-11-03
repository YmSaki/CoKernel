"""Privileged disposable Linux fixture for non-root Runtime/workload separation.

Run as root ONLY to prepare/remove this script's fresh /tmp fixture and assign
two unused numeric IDs. No accounts, system config or existing files are changed.
Runtime itself runs non-root with exactly kill/setgid/setuid file capabilities.
Requires built Runtime/MCP, uv, setcap, and the verified tunnel-client argument.
Uses a local provider and synthetic credentials, never a real external tunnel.
"""
from __future__ import annotations
import grp
import json
import os
from pathlib import Path
import pwd
import secrets
import shutil
import signal
import subprocess
import sys
import tempfile
import threading
import time

from smoke_notebook_runtime import Bridge, ROOT, import_notebook, wait_operation, wait_package
from smoke_tunnel_supervision import KEY, TUNNEL_ID, Plane, status_until


def unused_id():
    while True:
        value = 200000 + secrets.randbelow(10000000)
        try:
            pwd.getpwuid(value)
        except KeyError:
            try:
                grp.getgrgid(value)
            except KeyError:
                return value


def main():
    assert sys.platform == "linux" and os.geteuid() == 0, "root fixture setup required"
    tunnel_source = Path(sys.argv[1]).resolve(strict=True)
    python_source = Path(sys.argv[2]).resolve(strict=True)
    uv = shutil.which("uv")
    assert uv and shutil.which("setcap")
    plane = Plane()
    threading.Thread(target=plane.serve_forever, daemon=True).start()
    try:
        with tempfile.TemporaryDirectory(prefix="ckuid-") as temporary:
            root = Path(temporary)
            control = unused_id()
            workload = unused_id()
            assert control != workload
            root.chmod(0o711)
            def directory(name, owner, group, mode):
                path = root / name
                path.mkdir(parents=True)
                os.chown(path, owner, group)
                path.chmod(mode)
                return path
            binaries = directory("bin", 0, 0, 0o755)
            state = directory("state", control, control, 0o700)
            projects = directory("projects", control, workload, 0o2770)
            sockets = directory("s", control, workload, 0o2710)
            cache = directory("cache", workload, workload, 0o2770)
            python = directory("python", workload, workload, 0o2770)
            # Use a copied known Python distribution; the host distro's system
            # Python can be older than the worker's supported minimum.
            installed_python = python / python_source.name
            shutil.copytree(python_source, installed_python, symlinks=True)
            for path in [installed_python, *installed_python.rglob("*")]:
                if not path.is_symlink():
                    path.chmod(0o755 if path.is_dir() or path.stat().st_mode & 0o111 else 0o644)
            (binaries / "python3").symlink_to(installed_python / "bin/python3.12")
            (binaries / "python").symlink_to(installed_python / "bin/python3.12")
            def binary(source, name, group=0, mode=0o755):
                target = binaries / name
                shutil.copyfile(source, target)
                os.chown(target, 0, group)
                target.chmod(mode)
                return target
            runtime = binary(ROOT / "target/debug/cokernel-runtime", "cokernel-runtime", control, 0o750)
            mcp = binary(ROOT / "target/debug/cokernel-mcp", "cokernel-mcp", control, 0o750)
            tunnel = binary(tunnel_source, "tunnel-client", control, 0o750)
            real_uv = binary(uv, "uv-real")
            uv_wrapper = binaries / "uv"
            uv_wrapper.write_text(f'#!/bin/sh\nexec "{real_uv}" "$@" 2> "$PWD/.uv-proof-stderr"\n')
            uv_wrapper.chmod(0o755)
            subprocess.run(["setcap", "cap_kill,cap_setgid,cap_setuid=ep", str(runtime)], check=True)
            shutil.copytree(ROOT / "worker", root / "worker", ignore=shutil.ignore_patterns(
                ".venv", "__pycache__", "*.egg-info", ".pytest_cache", ".ruff_cache"))
            for path in [root / "worker", *(root / "worker").rglob("*")]:
                if not path.is_symlink():
                    path.chmod(0o755 if path.is_dir() else 0o644)
            wrapper = binaries / "tunnel"
            wrapper.write_text("#!/usr/bin/python3\nimport os,sys\na=sys.argv[1:]\n"
                "i=a.index('--control-plane.base-url')+1\n"
                f"a[i]='http://127.0.0.1:{plane.server_port}'\n"
                f"os.execv({str(tunnel)!r},[{str(tunnel)!r}]+a)\n")
            os.chown(wrapper, 0, control)
            wrapper.chmod(0o750)
            secret = state / "control-secret"
            secret.write_text("synthetic-control-only-file")
            os.chown(secret, control, control)
            secret.chmod(0o600)
            bearer_value = secrets.token_hex(32)
            bearer_path = state / "bearer"
            bearer_path.write_text(bearer_value)
            os.chown(bearer_path, control, control)
            bearer_path.chmod(0o600)
            env = {
                "PATH": f"{binaries}:/usr/bin:/bin", "HOME": str(state), "LC_ALL": "C.UTF-8",
                "UV_CACHE_DIR": str(cache), "UV_PYTHON_INSTALL_DIR": str(python),
                "COKERNEL_WORKLOAD_UID": str(workload), "COKERNEL_WORKLOAD_GID": str(workload),
                "COKERNEL_PROJECTS_ROOT": str(projects), "COKERNEL_PROJECT_REGISTRY": str(state / "projects.json"),
                "COKERNEL_WORKER_PACKAGE": str(root / "worker"), "COKERNEL_SESSION_SOCKET_DIR": str(sockets),
                "COKERNEL_FAILURE_DIR": str(state / "failures"), "COKERNEL_RUNTIME_SOCKET": str(state / "runtime.sock"),
                "COKERNEL_MCP_EXECUTABLE": str(mcp), "COKERNEL_MCP_BEARER_FILE": str(state / "bearer"),
                "COKERNEL_MCP_PORT": "0", "COKERNEL_TUNNEL_EXECUTABLE": str(wrapper),
                "COKERNEL_TUNNEL_STATE_DIR": str(state), "COKERNEL_CONTROL_SENTINEL": "synthetic-control-env",
            }
            options = dict(user=control, group=control, extra_groups=[workload], umask=0o007)
            # Missing/partial/root/same-user targets must fail without executing uv.
            for replacement in [
                {"COKERNEL_WORKLOAD_UID": None, "COKERNEL_WORKLOAD_GID": None},
                {"COKERNEL_WORKLOAD_UID": None}, {"COKERNEL_WORKLOAD_UID": "0"},
                {"COKERNEL_WORKLOAD_UID": str(control)},
                {"COKERNEL_WORKLOAD_GID": str(control)},
                {"COKERNEL_WORKLOAD_UID": "4294967295"},
                {"COKERNEL_WORKLOAD_GID": "invalid"},
            ]:
                bad = dict(env)
                for name, value in replacement.items():
                    if value is None: bad.pop(name, None)
                    else: bad[name] = value
                result = subprocess.run([str(runtime), "project", "list"], env=bad, **options,
                    stdout=subprocess.PIPE, stderr=subprocess.PIPE, timeout=10)
                assert result.returncode != 0 and b"CK-SEC-WORKLOAD" in result.stderr
            assert not (state / "projects.json").exists()
            projects.chmod(0o770)
            refused = subprocess.run([str(runtime), "project", "list"], env=env, **options,
                capture_output=True, timeout=10)
            assert refused.returncode != 0 and b"CK-SEC-WORKLOAD" in refused.stderr
            projects.chmod(0o2770)
            subprocess.run(["setcap", "cap_setgid,cap_setuid=ep", str(runtime)], check=True)
            refused = subprocess.run([str(runtime), "project", "list"], env=env, **options,
                capture_output=True, timeout=10)
            assert refused.returncode != 0 and b"CK-SEC-WORKLOAD" in refused.stderr
            subprocess.run(["setcap", "cap_kill,cap_setgid,cap_setuid=ep", str(runtime)], check=True)
            observed_frames = []
            def observe(value):
                encoded = json.dumps(value)
                assert KEY not in encoded, "provider key in public frame"
                assert bearer_value not in encoded, "MCP bearer in public frame"
                # Bounded metadata only; never retain notebook output or keys.
                observed_frames.append("event" if "event" in value else "response")
                assert len(observed_frames) <= 4096
            bridge = Bridge(env, root, executable=runtime, process_options=options, observer=observe)
            try:
                bridge.rpc("tunnel.start", tunnel_id=TUNNEL_ID, runtime_key=KEY)
                ready = status_until(bridge, lambda s: s["state"] == "CONNECTED", seconds=30)
                mcp_status = bridge.rpc("runtime.get_mcp_status")
                control_pids = [bridge.process.pid, ready["pid"], mcp_status["pid"]]
                for index, pid in enumerate(control_pids):
                    status = dict(line.split(":", 1) for line in Path(f"/proc/{pid}/status").read_text().splitlines() if ":" in line)
                    assert set(map(int, status["Uid"].split())) == {control}
                    assert int(status["CapEff"].strip(), 16) == (0xe0 if index == 0 else 0)
                    assert int(status["CapInh"].strip(), 16) == int(status["CapAmb"].strip(), 16) == 0
                configuration = {"uid": workload, "gid": workload, "control_pids": control_pids,
                    "files": [str(secret), str(state / "bearer")], "socket": str(state / "runtime.sock")}
                package = directory("package", 0, 0, 0o755)
                probe = '''import json, os, socket
from pathlib import Path
CONFIG = json.loads(Path(__file__).with_name('probe.json').read_text())
def inspect():
    status = dict(line.split(':', 1) for line in Path('/proc/self/status').read_text().splitlines() if ':' in line)
    assert os.getresuid() == (CONFIG['uid'],) * 3
    assert os.getresgid() == (CONFIG['gid'],) * 3
    assert os.getgroups() == []
    assert all(int(status[name].strip(), 16) == 0 for name in ['CapInh','CapPrm','CapEff','CapAmb'])
    assert status['NoNewPrivs'].strip() == '1'
    assert 'COKERNEL_CONTROL_SENTINEL' not in os.environ
    denied = []
    for path in CONFIG['files'] + [f'/proc/{pid}/environ' for pid in CONFIG['control_pids']]:
        try:
            Path(path).read_bytes()
        except PermissionError:
            denied.append(path)
        else:
            raise AssertionError('control read was permitted')
    with socket.socket(socket.AF_UNIX) as sock:
        try: sock.connect(CONFIG['socket'])
        except PermissionError: pass
        else: raise AssertionError('control socket was accessible')
    for pid in CONFIG['control_pids']:
        try: os.kill(pid, 0)
        except PermissionError: pass
        else: raise AssertionError('control process signal permission')
    try: os.setuid(0)
    except PermissionError: pass
    else: raise AssertionError('regained root')
    return {'uid': os.getuid(), 'gid': os.getgid(), 'read_denials': len(denied), 'no_new_privs': True}
if __name__ == '__main__': print(json.dumps(inspect()))
'''
                (package / "identity_probe.py").write_text(probe)
                (package / "probe.json").write_text(json.dumps(configuration))
                (package / "pyproject.toml").write_text('[build-system]\nrequires=[]\nbuild-backend="backend"\nbackend-path=["."]\n[project]\nname="cokernel-identity-proof"\nversion="0.1.0"\n')
                report = projects / "build-report.jsonl"
                hold = projects / "hold-build"
                building = projects / "build-live.json"
                (package / "backend.py").write_text("import json,subprocess,sys,zipfile,os,time\nfrom pathlib import Path\nfrom identity_probe import inspect\n"
                    "def get_requires_for_build_wheel(config_settings=None): return []\n"
                    "def build_wheel(wheel_directory,config_settings=None,metadata_directory=None):\n"
                    f" if Path({str(hold)!r}).exists():\n"
                    "  child=subprocess.Popen([sys.executable,'-c','import time; time.sleep(60)'])\n"
                    f"  Path({str(building)!r}).write_text(json.dumps([os.getpid(),child.pid]))\n"
                    f"  while Path({str(hold)!r}).exists(): time.sleep(.02)\n"
                    " result=inspect()\n"
                    " result['child']=json.loads(subprocess.check_output([sys.executable,str(Path(__file__).with_name('identity_probe.py'))]))\n"
                    f" with open({str(report)!r},'a') as f: f.write(json.dumps(result)+'\\n')\n"
                    " name='cokernel_identity_proof-0.1.0-py3-none-any.whl'\n"
                    " info='cokernel_identity_proof-0.1.0.dist-info/'\n"
                    " files={'cokernel_identity_proof.py':'answer=42\\n',info+'METADATA':'Metadata-Version: 2.1\\nName: cokernel-identity-proof\\nVersion: 0.1.0\\n',info+'WHEEL':'Wheel-Version: 1.0\\nGenerator: fixture\\nRoot-Is-Purelib: true\\nTag: py3-none-any\\n'}\n"
                    " files[info+'RECORD']=''.join(p+',,\\n' for p in [*files,info+'RECORD'])\n"
                    " with zipfile.ZipFile(Path(wheel_directory)/name,'w') as z:\n"
                    "  for p,v in files.items(): z.writestr(p,v)\n"
                    " return name\n")
                for path in package.iterdir(): path.chmod(0o644)
                created = bridge.rpc("projects.create", name="proof")
                project = wait_package(bridge, created["project_id"], created)["project"]["project_id"]
                added = bridge.rpc("packages.add", project_id=project, packages=[str(package)])
                wait_package(bridge, project, added)
                assert [json.loads(line) for line in report.read_text().splitlines()]
                source = f"import sys,subprocess,json,os\nsys.path.insert(0,{str(package)!r})\nfrom identity_probe import inspect\nidentity_observation=inspect()\nchild_observation=json.loads(subprocess.check_output([sys.executable,{str(package / 'identity_probe.py')!r}]))\nimport cokernel_identity_proof\nassert cokernel_identity_proof.answer==42\nsecret_from_master=123456789\nworker_pid=os.getpid()"
                diagnostic_secret = "diagnostic_project_secret_123456"
                cells = [("proof", source), ("wait", "import time\ntime.sleep(60)"),
                    ("descendant", "import subprocess,sys\nowned_child=subprocess.Popen([sys.executable,'-c','import time; time.sleep(60)'])\nchild_pid=owned_child.pid"),
                    (diagnostic_secret, "import os\nos._exit(29)")]
                document = import_notebook(bridge, project, "proof.ipynb", {"nbformat":4,"nbformat_minor":5,"metadata":{},
                    "cells":[{"cell_type":"code","id":cell,"source":code,"metadata":{},"execution_count":None,"outputs":[]} for cell,code in cells]})["document"]
                identity = {"project_id":project,"notebook_id":document["notebook_id"]}
                session = bridge.rpc("sessions.ensure_primary", **identity)["session_id"]
                live = {"project_id":project,"session_id":session}
                def execute(cell):
                    revision = bridge.rpc("notebooks.get", **identity)["revision"]
                    return bridge.rpc("sessions.execute_cell", **identity, session_id=session,cell_id=cell,expected_revision=revision)
                def value(name): return bridge.rpc("sessions.get_variable", **live,name=name)["value"]
                def complete(cell):
                    operation=execute(cell)
                    result=wait_operation(bridge,project,operation["operation_id"])
                    assert result["state"]=="PERSISTED" and result["execution_status"]=="SUCCEEDED",result
                complete("proof")
                worker=value("worker_pid")
                observation=value("identity_observation")
                assert observation["uid"]==workload and value("child_observation")==observation
                profile = projects / "proof/.cokernel/ipython"
                assert profile.is_dir() and profile.stat().st_uid == workload
                # Kernel-enforced separation must coexist with ordinary Project I/O.
                notebook_path=projects / "proof/proof.ipynb"
                assert notebook_path.stat().st_gid==workload and notebook_path.stat().st_mode & 0o777 == 0o660
                read = subprocess.run(["/usr/bin/python3","-c",f"from pathlib import Path; import json; json.loads(Path({str(notebook_path)!r}).read_text())"], user=workload,group=workload,extra_groups=[],capture_output=True)
                assert read.returncode==0,read.stderr
                operation=execute("wait")
                deadline=time.monotonic()+5
                while bridge.rpc("sessions.get", **live)["state"]!="EXECUTING":
                    assert time.monotonic()<deadline
                    time.sleep(.02)
                bridge.rpc("sessions.interrupt", **live)
                interrupted=wait_operation(bridge,project,operation["operation_id"])
                assert interrupted["execution_status"]=="INTERRUPTED", interrupted
                assert value("secret_from_master")==123456789
                complete("descendant")
                child=value("child_pid")
                bridge.rpc("sessions.stop", **live)
                deadline=time.monotonic()+10
                while Path(f"/proc/{worker}").exists() or Path(f"/proc/{child}").exists():
                    assert time.monotonic()<deadline,"owned descendants survived stop"
                    time.sleep(.03)
                bridge.rpc("sessions.restart", **identity)
                complete("proof")
                crash=execute(diagnostic_secret)
                deadline=time.monotonic()+10
                while bridge.rpc("sessions.get", **live)["state"]!="CRASHED":
                    assert time.monotonic()<deadline
                    time.sleep(.03)
                records = [json.loads(path.read_text())["record"] for path in (state / "failures").glob("*.json")]
                assert any(record.get("session_id") == session for record in records),"missing durable crash evidence for Session"
                assert diagnostic_secret in json.dumps(records), "diagnostic corpus never entered raw evidence"
                diagnostic = bridge.rpc("runtime.get_diagnostics")
                assert diagnostic["sessions"]["managed_identity_configured"]
                item = next(item for item in diagnostic["sessions"]["items"] if item["session_id"] == session)
                assert item["state"] == "CRASHED" and item["last_failure"]["persisted"]
                assert item["last_failure"]["failure_id"] in [record["failure_id"] for record in records]
                exported = json.dumps(diagnostic)
                for excluded in [KEY, (state / "bearer").read_text(), diagnostic_secret, str(root)]:
                    assert excluded not in exported, "private text in diagnostics"
                assert len(exported.encode()) <= 128 * 1024
                # Cancellation must kill package build descendants under the
                # other UID, including a child holding inherited output open.
                hold.touch()
                cancelled_package = root / "cancel-package"
                shutil.copytree(package, cancelled_package)
                pyproject = cancelled_package / "pyproject.toml"
                pyproject.write_text(pyproject.read_text().replace("cokernel-identity-proof", "cokernel-identity-cancel"))
                operation = bridge.rpc("packages.add", project_id=project, packages=[str(cancelled_package)])
                deadline = time.monotonic() + 20
                while not building.exists():
                    assert time.monotonic() < deadline, "build did not enter cancellation fixture"
                    time.sleep(.03)
                build_pids = json.loads(building.read_text())
                assert bridge.rpc("operations.cancel", project_id=project,operation_id=operation["operation_id"])["accepted"]
                deadline = time.monotonic() + 10
                while bridge.rpc("operations.get",project_id=project,operation_id=operation["operation_id"])["result"]["state"] not in ["CANCELLED","FAILED"]:
                    assert time.monotonic() < deadline
                    time.sleep(.03)
                while any(Path(f"/proc/{pid}").exists() for pid in build_pids):
                    assert time.monotonic() < deadline, "build descendants survived cancellation"
                    time.sleep(.03)
                bridge.rpc("bridge.shutdown")
                bridge.process.wait(timeout=10)
                assert bridge.process.returncode==0
                assert all(not Path(f"/proc/{pid}").exists() for pid in control_pids)
                assert plane.errors.empty()
                bearer = (state / "bearer").read_bytes()
                for path in [root / "runtime.stderr", *(state / "failures").glob("*.json"), *projects.glob("*/*.ipynb")]:
                    data = path.read_bytes()
                    assert KEY.encode() not in data and bearer not in data, "product secret in logs/evidence/notebook"
                print("[workload-identity-smoke] PASS " + json.dumps({"control_uid":control,"workload_uid":workload,"probe":observation,
                    "actual_build_and_child":True,"worker_and_child":True,"private_files_proc_socket_denied":True,
                    "interrupt_stop_restart_crash_cleanup":True,"build_cancellation_cleanup":True,"document_group_access":True,
                    "diagnostic_projection":True,"secret_free_public_frames":len(observed_frames),"local_provider_only":True}))
            except Exception:
                for log in projects.glob("*/.uv-proof-stderr"):
                    print("FIXTURE_UV_DIAGNOSTIC", log.read_text(errors="replace")[-12000:], flush=True)
                raise
            finally:
                bridge.finish()
    finally:
        plane.shutdown()
        plane.server_close()


if __name__ == "__main__": main()
