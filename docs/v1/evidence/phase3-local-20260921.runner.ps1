$ErrorActionPreference = 'Stop'
$env:PATH = 'C:\Users\star\.cargo\bin;C:\Users\star\.local\bin;C:\Users\star\.cache\codex-runtimes\codex-primary-runtime\dependencies\native\powershell;' + $env:PATH
$env:COKERNEL_WSL_DISTRO = $null
$env:UV_PROJECT_ENVIRONMENT = $null
$env:UV_PYTHON = '3.12'
$global:CkGateWslLauncher = (Get-Command wsl.exe -CommandType Application | Select-Object -First 1).Source
# Development-only dispatch: actual WSL, actual check.sh, byte-identical source.
# Native Linux tools/venv avoid /mnt/c startup latency. Nothing is skipped.
function global:wsl.exe {
    if (($args -join ' ') -ne 'bash scripts/v1/check.sh') {
        throw "Unexpected WSL gate arguments: $args"
    }
    & $global:CkGateWslLauncher -d Ubuntu --exec /usr/bin/python3 /mnt/c/programing/CoKernel/target/dev-tools/compare-snapshot.py
    if ($LASTEXITCODE -ne 0) { throw 'Source identity failed before Linux gate' }
    & $global:CkGateWslLauncher -d Ubuntu --cd /home/ymsaki/.cache/cokernel-v1-dev/repo --exec /usr/bin/env PATH=/home/ymsaki/.cache/cokernel-v1-dev/repo/worker/.venv/bin:/home/ymsaki/.cache/cokernel-v1-dev/bin:/home/ymsaki/.cargo/bin:/usr/local/bin:/usr/bin:/bin UV_CACHE_DIR=/home/ymsaki/.cache/cokernel-v1-dev/uv-cache UV_PYTHON_INSTALL_DIR=/home/ymsaki/.cache/cokernel-v1-dev/python bash scripts/v1/check.sh
    $result = $LASTEXITCODE
    if ($result -eq 0) {
        & $global:CkGateWslLauncher -d Ubuntu --exec /usr/bin/python3 /mnt/c/programing/CoKernel/target/dev-tools/compare-snapshot.py
        $result = $LASTEXITCODE
    }
    $global:LASTEXITCODE = $result
}
& C:\programing\CoKernel\scripts\v1\check.ps1
