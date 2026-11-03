import hashlib
from pathlib import Path

source = Path('/mnt/c/programing/CoKernel')
snapshot = Path('/home/ymsaki/.cache/cokernel-v1-dev/repo')
paths = (source / 'target/dev-tools/snapshot-files.txt').read_text().splitlines()
for name in paths:
    a = hashlib.sha256((source / name).read_bytes()).digest()
    b = hashlib.sha256((snapshot / name).read_bytes()).digest()
    if a != b:
        raise SystemExit(f'SOURCE MISMATCH: {name}')
print(f'SOURCE IDENTITY PASS: {len(paths)} files')
