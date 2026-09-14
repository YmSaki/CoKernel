#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$ROOT"

command -v python3 >/dev/null 2>&1 || {
  echo '[v1-check-worker] python3 is required' >&2
  exit 127
}

echo '[v1-check-worker] WBS ledger consistency'
python3 scripts/v1/validate_work_status.py --check

echo '[v1-check-worker] WBS ledger tests'
python3 -m unittest discover -s scripts/v1 -p 'test_work_status.py' -q

echo '[v1-check-worker] Protocol fixtures'
python3 scripts/v1/validate_protocol_fixtures.py

echo '[v1-check-worker] Source-tree worker AF_UNIX smoke'
python3 scripts/v1/smoke_worker_source.py

# The checks above intentionally run before the uv dependency gate: they are
# useful source-tree diagnostics in constrained environments, but success there
# does not replace the canonical uv Project/overlay path below.
command -v uv >/dev/null 2>&1 || {
  echo '[v1-check-worker] uv is required for the canonical worker gate' >&2
  exit 127
}

echo '[v1-check-worker] Worker sync'
uv sync --project worker --dev

echo '[v1-check-worker] Worker version'
uv run --project worker cokernel-worker --version

echo '[v1-check-worker] Worker tests'
uv run --project worker pytest

echo '[v1-check-worker] Worker protocol subprocess smoke (uv environment)'
python3 scripts/v1/smoke_worker_protocol.py

echo '[v1-check-worker] uv Project + worker overlay spike'
python3 scripts/v1/spike_uv_worker_overlay.py

echo '[v1-check-worker] Worker SIGINT survival spike'
python3 scripts/v1/spike_worker_interrupt.py

echo '[v1-check-worker] PASS'
