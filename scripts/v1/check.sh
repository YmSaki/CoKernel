#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$ROOT"

echo '[v1-check] Worker/protocol gate'
"$ROOT/scripts/v1/check-worker.sh"

echo '[v1-check] Rust/runtime gate'
"$ROOT/scripts/v1/check-rust.sh"

echo '[v1-check] PASS'
