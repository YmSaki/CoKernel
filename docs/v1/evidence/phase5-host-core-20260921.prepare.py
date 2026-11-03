from pathlib import Path
import json
import uuid

base = Path('/home/ymsaki/.cache/cokernel-v1-dev')
run = base / ('host-proof-' + uuid.uuid4().hex[:12])
run.mkdir(mode=0o700)
wrapper = run / 'runtime'
wrapper.write_text('''#!/bin/sh
export PATH=/home/ymsaki/.cache/cokernel-v1-dev/repo/worker/.venv/bin:/home/ymsaki/.cache/cokernel-v1-dev/bin:/home/ymsaki/.cargo/bin:/usr/local/bin:/usr/bin:/bin
export UV_CACHE_DIR=/home/ymsaki/.cache/cokernel-v1-dev/uv-cache
export UV_PYTHON_INSTALL_DIR=/home/ymsaki/.cache/cokernel-v1-dev/python
export COKERNEL_WORKER_PACKAGE=/home/ymsaki/.cache/cokernel-v1-dev/repo/worker
export COKERNEL_PROJECTS_ROOT=''' + str(run / 'projects') + '''
export COKERNEL_PROJECT_REGISTRY=''' + str(run / 'state/projects.json') + '''
export COKERNEL_SESSION_SOCKET_DIR=/tmp/ckhost-''' + run.name[-12:] + '''
export COKERNEL_FAILURE_DIR=''' + str(run / 'failures') + '''
exec /home/ymsaki/.cache/cokernel-v1-dev/repo/target/debug/cokernel-runtime "$@"
''')
wrapper.chmod(0o700)
config = {'distribution':'Ubuntu','linux_user':'ymsaki','runtime_path':str(wrapper)}
Path('/mnt/c/programing/CoKernel/target/dev-tools/host-wsl.json').write_text(json.dumps(config))
Path('/mnt/c/programing/CoKernel/target/dev-tools/host-proof-root.txt').write_text(str(run))
print('Prepared isolated Host proof:', run)
