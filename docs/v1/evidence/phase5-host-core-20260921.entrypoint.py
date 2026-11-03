import json
import subprocess
import tempfile
from pathlib import Path

root = Path('C:/programing/CoKernel')
with tempfile.TemporaryDirectory(prefix='ckhost-entry-') as state:
    process = subprocess.Popen([str(root / 'target/debug/cokernel-host.exe'),
                                'serve', str(root / 'target/dev-tools/host-wsl.json'), state, '--stop'],
                               stdin=subprocess.DEVNULL, stdout=subprocess.PIPE, stderr=subprocess.PIPE,
                               creationflags=subprocess.CREATE_NO_WINDOW)
    try:
        process.communicate(timeout=1)
        raise AssertionError('Host service exited instead of remaining alive')
    except subprocess.TimeoutExpired:
        process.terminate()
        stdout, stderr = process.communicate(timeout=5)
        records = [json.loads(line) for line in stdout.decode().splitlines()]
        assert records and records[0]['desired'] == 'STOPPED'
        assert all(r['generation'] == 0 for r in records)
        assert json.loads((Path(state) / 'desired.json').read_text())['desired'] == 'STOPPED'
        assert not stderr, stderr
    finally:
        if process.poll() is None:
            process.kill()
            process.wait(timeout=5)
print('PASS: real Windows Host service entrypoint remains alive in persisted STOPPED state; no Runtime launched')
