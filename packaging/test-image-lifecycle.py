"""Root-only CI test of maintenance against the actual packaged Linux image.
A chroot fixture supplies mount metadata; this is not a WSL/GPU acceptance test.
"""
import hashlib
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile
import uuid

assert os.geteuid() == 0, 'Run in disposable Linux CI with sudo'
image = Path(sys.argv[1]).resolve()
script = Path(__file__).with_name('runtime-maintenance.sh').read_bytes()
with tempfile.TemporaryDirectory(prefix='cokernel-image-test-') as directory:
    root = Path(directory)
    subprocess.run(['tar', '-xpf', str(image), '-C', str(root)], check=True)
    (root / 'run/WSL').mkdir(parents=True, exist_ok=True)
    (root / 'run/WSL').chmod(0o700)
    (root / 'proc/self').mkdir(parents=True, exist_ok=True)
    (root / 'proc/self/mountinfo').write_text(Path('/proc/self/mountinfo').read_text())
    (root / 'dev').mkdir(exist_ok=True)
    (root / 'dev/null').touch()
    manifest_path = root / 'etc/cokernel-validation.json'
    old = json.loads(manifest_path.read_text())
    subprocess.run(['chroot', str(root), '/opt/cokernel/bin/cokernel-bootstrap', 'finalize', old['identity']], check=True)
    project = root / 'var/lib/cokernel/projects/preserved'
    project.mkdir()
    for name, value in {'work.ipynb': '{"cells":[],"nbformat":4}', 'uv.lock': 'locked-dependencies', 'pyproject.toml': '[project]\nname="preserved"', 'output.bin': 'computed-result'}.items():
        (project / name).write_text(value)
    original = {p.name: p.read_bytes() for p in project.iterdir()}
    def run(action, identity):
        subprocess.run(['chroot', str(root), '/bin/sh', '-s', '--', action, identity], input=script, check=True)
    def stage():
        run('prepare', old['identity'])
        staged = root / 'opt/.cokernel-maintenance/new'
        subprocess.run(['tar', '-xpf', str(image), '-C', str(staged), 'opt/cokernel', 'etc/cokernel-validation.json', 'etc/ssl/certs/ca-certificates.crt', 'etc/ssl/cert.pem'], check=True)
        manifest = json.loads((staged / 'etc/cokernel-validation.json').read_text())
        manifest['identity'] = str(uuid.uuid4())
        worker = next(path for path in manifest['files'] if path.startswith('opt/cokernel/worker/') and path.endswith('.py'))
        with (staged / worker).open('ab') as file:
            file.write(b'\n# lifecycle fixture payload revision\n')
        manifest['files'][worker] = hashlib.sha256((staged / worker).read_bytes()).hexdigest()
        (staged / 'etc/cokernel-validation.json').write_text(json.dumps(manifest))
        return manifest['identity']
    next_identity = stage()
    run('apply', next_identity)
    assert json.loads(manifest_path.read_text())['identity'] == next_identity
    run('rollback', old['identity'])
    assert json.loads(manifest_path.read_text())['identity'] == old['identity']
    next_identity = stage()
    run('apply', next_identity)
    run('commit', next_identity)
    assert json.loads(manifest_path.read_text())['identity'] == next_identity
    assert original == {p.name: p.read_bytes() for p in project.iterdir()}
    assert not (root / 'opt/.cokernel-maintenance').exists()
print('Packaged-image update/rollback/commit, capability finalization and Project preservation passed.')
