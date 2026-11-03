"""Extend the disposable Host acceptance fixture with managed MCP supervision."""
from pathlib import Path
import runpy

root = Path('/mnt/c/programing/CoKernel')
runpy.run_path(str(root / 'docs/v1/evidence/phase5-host-core-20260921.prepare.py'))
directory = Path((root / 'target/dev-tools/host-proof-root.txt').read_text().strip())
wrapper = directory / 'runtime'
text = wrapper.read_text()
marker = 'exec /home/ymsaki/.cache/cokernel-v1-dev/repo/target/debug/cokernel-runtime'
assert text.count(marker) == 1
configuration = (
    f'export COKERNEL_RUNTIME_SOCKET={directory}/runtime.sock\n'
    'export COKERNEL_MCP_EXECUTABLE=/home/ymsaki/.cache/cokernel-v1-dev/repo/target/debug/cokernel-mcp\n'
    f'export COKERNEL_MCP_BEARER_FILE={directory}/bearer\n'
    'export COKERNEL_MCP_PORT=0\n'
)
wrapper.write_text(text.replace(marker, configuration + marker))
print('Prepared optional Runtime-owned MCP in isolated Host fixture')
