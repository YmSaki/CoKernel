"""Exercise actual filesystem transitions in an isolated path-rewritten fixture.
No WSL/GPU claim: Windows transport and installed-system acceptance are separate.
"""
from pathlib import Path
import re
import shutil
import subprocess
import tempfile

SOURCE = Path(__file__).with_name("runtime-maintenance.sh").read_text()

def scenario(fault=None):
    with tempfile.TemporaryDirectory(prefix="ck-update-") as directory:
        root = Path(directory)
        for name in ("opt/cokernel/bin", "etc/ssl/certs", "var/lib/cokernel/projects/demo"):
            (root / name).mkdir(parents=True)
        notebook = root / "var/lib/cokernel/projects/demo/work.ipynb"
        notebook.write_bytes(b'{"cells": [], "nbformat": 4}')
        before = notebook.read_bytes()
        bootstrap = root / "opt/cokernel/bin/cokernel-bootstrap"
        bootstrap.write_text('#!/bin/sh\nexit 0\n')
        bootstrap.chmod(0o755)
        (root / "opt/cokernel/version").write_text("old")
        for name in ("cokernel-validation.json", "ssl/certs/ca-certificates.crt", "ssl/cert.pem"):
            (root / "etc" / name).write_text("old")
        # Rewrite ONLY absolute product roots and the UID assertion for unprivileged tests.
        script = SOURCE.replace('[ "$(id -u)" = 0 ] || exit 71', ':')
        script = re.sub(r'(?<![\w])/(opt|etc)/', lambda m: f'{root}/{m[1]}/', script)
        runner = root / "maintenance.sh"
        runner.write_text(script)
        def run(action, expected=0):
            result = subprocess.run(['sh', str(runner), action, 'fixture-identity'], capture_output=True)
            assert result.returncode == expected, (action, result.stderr.decode())
        run('prepare')
        run('prepare', 72)
        staged = root / 'opt/.cokernel-maintenance/new'
        shutil.copytree(root / 'opt/cokernel', staged / 'opt/cokernel')
        shutil.copytree(root / 'etc', staged / 'etc')
        (staged / 'opt/cokernel/version').write_text('new')
        for name in ("cokernel-validation.json", "ssl/certs/ca-certificates.crt", "ssl/cert.pem"):
            (staged / 'etc' / name).write_text('new')
        if fault:
            fault = re.sub(r'(?<![\w])/(opt|etc)/', lambda m: f'{root}/{m[1]}/', fault)
            assert fault in script
            runner.write_text(script.replace(fault, 'exit 93\n  ' + fault))
            run('apply', 93)
            runner.write_text(script)
            run('rollback')
            run('rollback')  # restart-safe, idempotent recovery
            assert (root / 'opt/cokernel/version').read_text() == 'old'
            assert (root / 'etc/cokernel-validation.json').read_text() == 'old'
        else:
            run('apply')
            assert (root / 'opt/cokernel/version').read_text() == 'new'
            run('commit')
            run('commit')
            assert not (root / 'opt/.cokernel-maintenance').exists()
        assert notebook.read_bytes() == before

scenario()
# Faults before/after switch, during metadata replacement, and before finalize.
for boundary in (
    'touch "$work/backed-up"',
    'mv /opt/cokernel "$work/displaced"',
    'mv "$work/new/opt/cokernel" /opt/cokernel',
    'cp -p "$work/new/etc/cokernel-validation.json" /etc/cokernel-validation.json',
    'cp -p "$work/new/etc/ssl/cert.pem" /etc/ssl/cert.pem',
    '/opt/cokernel/bin/cokernel-bootstrap finalize "$2"',
):
    scenario(boundary)
print('Runtime update, rollback and notebook preservation checks passed.')
