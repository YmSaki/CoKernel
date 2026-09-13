#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$ROOT"

echo '[v1-check] Rust format'
cargo fmt --all --check

echo '[v1-check] Rust check'
cargo check --workspace --all-targets

echo '[v1-check] Rust clippy'
cargo clippy --workspace --all-targets -- -D warnings

echo '[v1-check] Rust tests'
cargo test --workspace

echo '[v1-check] Protocol fixtures'
python3 scripts/v1/validate_protocol_fixtures.py

echo '[v1-check] Worker sync'
uv sync --project worker --dev

echo '[v1-check] Worker version'
uv run --project worker cokernel-worker --version

echo '[v1-check] Worker tests'
uv run --project worker pytest

echo '[v1-check] uv Project + worker overlay spike'
python3 scripts/v1/spike_uv_worker_overlay.py

echo '[v1-check] PASS'
