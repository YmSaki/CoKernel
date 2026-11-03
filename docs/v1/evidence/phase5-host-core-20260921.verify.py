from pathlib import Path
import json

source = Path('/mnt/c/programing/CoKernel')
result = json.loads((source / 'target/dev-tools/phase5-real-host.log').read_text(encoding='utf-8-sig').strip())
pid = result['worker_pid']
assert isinstance(pid, int) and pid > 1
assert not Path(f'/proc/{pid}').exists(), 'worker still exists after Host stop'
root = Path((source / 'target/dev-tools/host-proof-root.txt').read_text().strip())
notebooks = list((root / 'projects').rglob('host.ipynb'))
assert len(notebooks) == 1
notebook = json.loads(notebooks[0].read_text())
assert notebook['metadata']['shutdown_flush'] is True
assert notebook['cells'][0]['outputs'][0]['data']['text/plain'] == '124'
print('PASS: real WSL worker exited after Host stop; saved notebook output and shutdown edit retained')
