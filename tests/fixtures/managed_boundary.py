"""Executed as a notebook cell through Host, never as a root setup probe."""
import errno
import os
from pathlib import Path
import socket
import subprocess


def managed_boundary_proof():
    assert os.getresuid() == (61002, 61002, 61002)
    assert os.getresgid() == (61002, 61002, 61002)
    assert os.getgroups() == []
    status = dict(line.split(":", 1) for line in Path("/proc/self/status").read_text().splitlines())
    for field in ("CapInh", "CapPrm", "CapEff", "CapAmb"):
        assert int(status[field].strip(), 16) == 0, field
    assert status["NoNewPrivs"].strip() == "1"
    assert str(Path.cwd()).startswith("/var/lib/cokernel/projects/")
    assert not any(key.startswith(("COKERNEL_", "TUNNEL_")) for key in os.environ)
    assert "WSL_INTEROP" not in os.environ
    mounts = Path("/proc/self/mountinfo").read_text()
    assert " - drvfs " not in mounts and "aname=drvfs" not in mounts
    assert not Path("/mnt/c/Windows").exists()
    # uv can be the worker's immediate parent under the workload identity.
    # Find the actual live control Runtime, rather than assuming parent == Runtime.
    runtimes = []
    for directory in Path("/proc").iterdir():
        if not directory.name.isdecimal():
            continue
        try:
            process = dict(line.split(":", 1) for line in (directory / "status").read_text().splitlines())
        except FileNotFoundError:
            continue
        if process["Name"].strip() == "cokernel-runtim" and process["Uid"].split() == ["61001"] * 4:
            runtimes.append(directory)
    assert len(runtimes) == 1, "expected one live managed control Runtime"
    for name in ("/var/lib/cokernel/state/bearer", "/etc/shadow",
                 str(runtimes[0] / "environ")):
        try:
            with open(name, "rb"):
                pass
        except PermissionError:
            pass
        else:
            raise AssertionError(f"protected file readable: {name}")
    for name in ("/run/WSL/1_interop", "/var/lib/cokernel/state/runtime.sock"):
        with socket.socket(socket.AF_UNIX) as client:
            try:
                client.connect(name)
            except PermissionError:
                pass
            else:
                raise AssertionError(f"protected socket accessible: {name}")
    try:
        os.setuid(0)
    except PermissionError:
        pass
    else:
        raise AssertionError("workload regained root")
    probe = "/var/lib/cokernel/windows-execution-probe.exe"
    assert Path(probe).read_bytes()[:2] == b"MZ", "Windows probe not staged"
    for injected in (False, True):
        environment = dict(os.environ)
        if injected:
            environment["WSL_INTEROP"] = "/run/WSL/1_interop"
        try:
            result = subprocess.run([probe], env=environment, capture_output=True, timeout=5)
        except OSError as error:
            assert error.errno in (errno.ENOEXEC, errno.EACCES)
        else:
            assert result.returncode != 0 and not result.stdout, "Windows execution succeeded"


managed_boundary_proof()
del managed_boundary_proof
