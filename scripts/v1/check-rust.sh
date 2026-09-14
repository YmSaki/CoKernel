#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$ROOT"

command -v cargo >/dev/null 2>&1 || {
  echo '[v1-check-rust] cargo is required' >&2
  exit 127
}

echo '[v1-check-rust] Rust format'
cargo fmt --all --check

echo '[v1-check-rust] Rust check'
cargo check --workspace --all-targets

echo '[v1-check-rust] Rust clippy'
cargo clippy --workspace --all-targets -- -D warnings

echo '[v1-check-rust] Rust tests'
cargo test --workspace

echo '[v1-check-rust] Runtime Project + uv smoke'
python3 scripts/v1/smoke_runtime_project.py

echo '[v1-check-rust] Session Supervisor canonical smoke'
cargo run -q -p cokernel-runtime --example session_supervisor_smoke

echo '[v1-check-rust] Session safe-inspection smoke'
cargo run -q -p cokernel-runtime --example session_inspection_smoke

echo '[v1-check-rust] PASS'
