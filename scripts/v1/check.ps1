$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest

function Assert-NativeSuccess([string]$Step) {
    if ($LASTEXITCODE -ne 0) {
        throw "$Step failed with exit code $LASTEXITCODE"
    }
}

$Root = (Resolve-Path (Join-Path $PSScriptRoot '..\..')).Path
Push-Location $Root
try {
    foreach ($RequiredCommand in @('python', 'cargo', 'uv')) {
        if ($null -eq (Get-Command $RequiredCommand -ErrorAction SilentlyContinue)) {
            throw "$RequiredCommand is required for the native Windows verification slice"
        }
    }

    Write-Host '[v1-check] Environment'
    python --version
    Assert-NativeSuccess 'Python version'
    cargo --version
    Assert-NativeSuccess 'Cargo version'
    uv --version
    Assert-NativeSuccess 'uv version'

    Write-Host '[v1-check] WBS ledger consistency'
    python scripts/v1/validate_work_status.py --check
    Assert-NativeSuccess 'WBS ledger consistency'

    Write-Host '[v1-check] Local v1 script contract tests'
    python -m unittest discover -s scripts/v1 -p 'test_*.py' -q
    Assert-NativeSuccess 'Local script contract tests'

    Write-Host '[v1-check] Protocol fixtures'
    python scripts/v1/validate_protocol_fixtures.py
    Assert-NativeSuccess 'Protocol fixtures'

    Write-Host '[v1-check] Source-tree worker smoke (Linux/WSL proof; skips on native Windows)'
    python scripts/v1/smoke_worker_source.py
    Assert-NativeSuccess 'Source-tree worker smoke'

    Write-Host '[v1-check] Rust format'
    cargo fmt --all --check
    Assert-NativeSuccess 'Rust format'

    Write-Host '[v1-check] Rust check'
    cargo check --workspace --all-targets
    Assert-NativeSuccess 'Rust check'

    Write-Host '[v1-check] Rust clippy'
    cargo clippy --workspace --all-targets -- -D warnings
    Assert-NativeSuccess 'Rust clippy'

    Write-Host '[v1-check] Rust tests'
    cargo test --workspace
    Assert-NativeSuccess 'Rust tests'

    Write-Host '[v1-check] Worker sync'
    uv sync --project worker --dev
    Assert-NativeSuccess 'Worker sync'

    Write-Host '[v1-check] Worker version'
    uv run --project worker cokernel-worker --version
    Assert-NativeSuccess 'Worker version'

    Write-Host '[v1-check] Worker tests'
    uv run --project worker pytest worker/tests
    Assert-NativeSuccess 'Worker tests'

    Write-Host '[v1-check] Worker protocol subprocess smoke (Linux/WSL proof; skips on native Windows)'
    python scripts/v1/smoke_worker_protocol.py
    Assert-NativeSuccess 'Worker protocol smoke'

    Write-Host '[v1-check] uv Project + worker overlay spike'
    python scripts/v1/spike_uv_worker_overlay.py
    Assert-NativeSuccess 'Project worker overlay'

    Write-Host '[v1-check] Worker SIGINT survival spike (Linux/WSL proof; skips on native Windows)'
    python scripts/v1/spike_worker_interrupt.py
    Assert-NativeSuccess 'Worker interrupt smoke'

    Write-Host '[v1-check] Runtime Project + uv smoke'
    python scripts/v1/smoke_runtime_project.py
    Assert-NativeSuccess 'Runtime Project smoke'

    # The production Runtime/worker path is Linux inside WSL. Native Windows
    # checks alone cannot execute the Unix Session Supervisor, peer-credential,
    # SIGINT, or Unix-socket acceptance gates. Re-run the authoritative shell
    # gate inside WSL before reporting PASS so check.ps1 cannot produce a false
    # green result after the Windows-only portions skipped production-path proof.
    Write-Host '[v1-check] WSL production-path gate'
    $Wsl = Get-Command 'wsl.exe' -ErrorAction SilentlyContinue
    if ($null -eq $Wsl) {
        throw 'wsl.exe is required to verify the CoKernel v1 production Runtime path'
    }

    $WslArgs = @()
    if (-not [string]::IsNullOrWhiteSpace($env:COKERNEL_WSL_DISTRO)) {
        $WslArgs += @('--distribution', $env:COKERNEL_WSL_DISTRO)
    }
    $WslArgs += @('bash', 'scripts/v1/check.sh')

    & wsl.exe @WslArgs
    if ($LASTEXITCODE -ne 0) {
        throw "WSL production-path gate failed with exit code $LASTEXITCODE"
    }

    Write-Host '[v1-check] PASS'
}
finally {
    Pop-Location
}
