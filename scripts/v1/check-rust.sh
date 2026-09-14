#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$ROOT"

require_command() {
  local command_name="$1"
  command -v "$command_name" >/dev/null 2>&1 || {
    echo "[v1-check-rust] ${command_name} is required" >&2
    exit 127
  }
}

require_command cargo
require_command rustc
require_command rustfmt
require_command python3
require_command uv

# Clippy is a rustup component rather than a standalone prerequisite on every
# installation. Probe it explicitly before beginning the gate so a machine
# cannot pass format/check and then fail later for an incomplete toolchain.
cargo clippy --version >/dev/null 2>&1 || {
  echo '[v1-check-rust] cargo clippy is required' >&2
  exit 127
}

echo '[v1-check-rust] Toolchain'
echo "  $(rustc --version)"
echo "  $(cargo --version)"
echo "  $(rustfmt --version)"
echo "  $(uv --version)"
echo "  $(python3 --version)"

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
