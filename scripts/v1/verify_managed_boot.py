"""Root-only negative launch proof in an owned, idle validation distro.

Usage via WSL stdin: python3.12 - <expected-image-uuid>
Never touches VM-wide binfmt state or a different distribution.
"""
import json
import os
from pathlib import Path
import subprocess
import sys
import uuid


def main():
    assert os.geteuid() == 0
    identity = str(uuid.UUID(sys.argv[1]))
    manifest = json.loads(Path("/etc/cokernel-validation.json").read_text())
    assert manifest["identity"] == identity
    for directory in Path("/proc").iterdir():
        if not directory.name.isdecimal():
            continue
        try:
            status = dict(line.split(":", 1) for line in (directory / "status").read_text().splitlines())
        except FileNotFoundError:
            continue
        assert not {"61001", "61002"}.intersection(status["Uid"].split()), "live product process"
    bootstrap = "/opt/cokernel/bin/cokernel-bootstrap"
    before = subprocess.run([bootstrap, "inspect"], check=True, capture_output=True, timeout=30)
    assert json.loads(before.stdout)["identity"] == identity
    guard = Path("/run/WSL")
    try:
        guard.chmod(0o755)
        result = subprocess.run(
            ["/opt/cokernel/launch"], input=b"", capture_output=True, timeout=10,
            user=61001, group=61001, extra_groups=[61002], cwd="/",
            env={"PATH": "/usr/bin:/bin"},
        )
        assert result.returncode != 0 and not result.stdout
        assert b"CK-SETUP-POLICY" in result.stderr, "launch did not fail at the boundary check"
    finally:
        subprocess.run([bootstrap, "boot-guard"], check=True, timeout=10)
    subprocess.run([bootstrap, "inspect"], check=True, capture_output=True, timeout=30)
    print(json.dumps({"identity": identity, "idle_before_mutation": True,
                      "unsafe_interop_permissions_reject_launch": True,
                      "boot_guard_restores_boundary": True}))


if __name__ == "__main__":
    main()
