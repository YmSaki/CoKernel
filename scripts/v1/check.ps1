$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest

$Root = (Resolve-Path (Join-Path $PSScriptRoot '..\..')).Path
Push-Location $Root
try {
    Write-Host '[v1-check] Rust format'
    cargo fmt --all --check

    Write-Host '[v1-check] Rust check'
    cargo check --workspace --all-targets

    Write-Host '[v1-check] Rust clippy'
    cargo clippy --workspace --all-targets -- -D warnings

    Write-Host '[v1-check] Rust tests'
    cargo test --workspace

    Write-Host '[v1-check] Protocol fixtures'
    python scripts/v1/validate_protocol_fixtures.py

    Write-Host '[v1-check] Worker sync'
    uv sync --project worker --dev

    Write-Host '[v1-check] Worker version'
    uv run --project worker cokernel-worker --version

    Write-Host '[v1-check] Worker tests'
    uv run --project worker pytest

    Write-Host '[v1-check] PASS'
}
finally {
    Pop-Location
}
