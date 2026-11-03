"""Private local provider and live /proc checks for native Host/Secret Store proof."""
from pathlib import Path
import json
import os
import runpy
import sys
import tempfile
import threading
import time

root = Path('/mnt/c/programing/CoKernel')
repo = Path('/home/ymsaki/.cache/cokernel-v1-dev/repo')
sys.path.insert(0, str(repo / 'scripts/v1'))
from smoke_tunnel_supervision import Plane, KEY

runpy.run_path(str(root / 'docs/v1/evidence/phase6-mcp-supervision-20260922.prepare.py'))
directory = Path((root / 'target/dev-tools/host-proof-root.txt').read_text().strip())
plane = Plane()
thread = threading.Thread(target=plane.serve_forever, daemon=True)
thread.start()
state = tempfile.TemporaryDirectory(prefix='ckhost-tun-')
wrapper = directory / 'tunnel'
binary = '/home/ymsaki/.cache/cokernel-v1-dev/tunnel-client-v0.0.14/tunnel-client'
wrapper.write_text("#!/usr/bin/python3\nimport os,sys\na=sys.argv[1:]\ni=a.index('--control-plane.base-url')+1\n"
    f"a[i]='http://127.0.0.1:{plane.server_port}'\nos.execv({binary!r},[{binary!r}]+a)\n")
wrapper.chmod(0o700)
runtime = directory / 'runtime'
text = runtime.read_text()
marker = 'exec /home/ymsaki/.cache/cokernel-v1-dev/repo/target/debug/cokernel-runtime'
text = text.replace(marker, f'export COKERNEL_TUNNEL_EXECUTABLE={wrapper}\nexport COKERNEL_TUNNEL_STATE_DIR={state.name}\necho $$ > {directory}/runtime.pid\n'+marker)
runtime.write_text(text)
print('HOST_TUNNEL_PROVIDER_READY', flush=True)
seen_workers = set()
seen_tunnels = set()
deadline = time.monotonic() + 600
try:
    while time.monotonic() < deadline and not (directory / 'provider.stop').exists():
        try:
            pid = int((directory / 'runtime.pid').read_text())
            pending = [pid]
            visited = set()
            while pending:
                child = pending.pop()
                if child in visited:
                    continue
                visited.add(child)
                for children in Path(f'/proc/{child}/task').glob('*/children'):
                    pending.extend(map(int, children.read_text().split()))
                args = Path(f'/proc/{child}/cmdline').read_bytes()
                if b'cokernel-worker' in args:
                    env = Path(f'/proc/{child}/environ').read_bytes()
                    bearer = (directory / 'bearer').read_bytes()
                    assert KEY.encode() not in env and bearer not in env
                    assert KEY.encode() not in args and bearer not in args
                    seen_workers.add(child)
                if binary.encode() in args:
                    env = Path(f'/proc/{child}/environ').read_bytes()
                    bearer = (directory / 'bearer').read_bytes()
                    assert KEY.encode() in env and bearer in env
                    assert KEY.encode() not in args and bearer not in args
                    seen_tunnels.add(child)
        except (FileNotFoundError, ProcessLookupError):
            pass
        time.sleep(.05)
    assert (directory / 'provider.stop').exists(), 'native proof did not signal completion'
    assert seen_workers and seen_tunnels
    assert plane.errors.empty(), list(plane.errors.queue)
    assert all(not Path(f'/proc/{pid}').exists() for pid in seen_workers | seen_tunnels)
    report = dict(worker_pids=sorted(seen_workers), tunnel_pids=sorted(seen_tunnels), credential_only_in_tunnel=True, all_children_gone=True)
    (root / 'docs/v1/evidence/phase7-host-handoff-20260922.postconditions.json').write_text(json.dumps(report, indent=2)+'\n')
    print('HOST_TUNNEL_POSTCONDITIONS_PASS', json.dumps(report), flush=True)
finally:
    plane.shutdown()
    plane.server_close()
    thread.join(timeout=3)
    state.cleanup()
