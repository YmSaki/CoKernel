[CmdletBinding()]
param(
    [string]$DistroName = "CoKernel",
    [switch]$AfterPull
)

$ErrorActionPreference = "Stop"
Set-StrictMode -Version Latest

function Invoke-Checked {
    param(
        [Parameter(Mandatory = $true)][scriptblock]$Command,
        [Parameter(Mandatory = $true)][string]$FailureMessage
    )

    & $Command
    if ($LASTEXITCODE -ne 0) {
        throw "$FailureMessage (exit code $LASTEXITCODE)."
    }
}

if (-not $AfterPull) {
    Write-Host "CoKernel update" -ForegroundColor Green
    Write-Host "  Windows checkout: $PSScriptRoot"
    Write-Host "  WSL distro:       $DistroName"
    Write-Host ""

    $trackedChanges = @(& git.exe -C $PSScriptRoot status --porcelain --untracked-files=no)
    if ($LASTEXITCODE -ne 0) {
        throw "Could not inspect the CoKernel Git checkout."
    }
    if ($trackedChanges.Count -gt 0) {
        Write-Host "Tracked local changes would make an automatic update unsafe:" -ForegroundColor Yellow
        $trackedChanges | ForEach-Object { Write-Host "  $_" }
        throw "Commit, stash, or restore the tracked changes, then rerun update.cmd."
    }

    Write-Host "==> Pulling latest CoKernel"
    & git.exe -C $PSScriptRoot pull --ff-only
    if ($LASTEXITCODE -ne 0) {
        throw "git pull --ff-only failed."
    }

    # Re-exec the updater from disk after git pull so migrations added by the
    # newly pulled version take effect during this same update invocation.
    & powershell.exe -NoProfile -ExecutionPolicy Bypass -File $PSCommandPath -DistroName $DistroName -AfterPull
    exit $LASTEXITCODE
}

Write-Host "==> Synchronizing the Windows checkout into WSL"
& powershell.exe -NoProfile -ExecutionPolicy Bypass -File (Join-Path $PSScriptRoot "sync-wsl-repo.ps1") -DistroName $DistroName
if ($LASTEXITCODE -ne 0) {
    throw "Could not synchronize the current checkout into the CoKernel WSL distro."
}

& wsl.exe -d $DistroName -u root --cd / -- test -f /etc/cokernel-managed
if ($LASTEXITCODE -ne 0) {
    throw "A managed CoKernel WSL distro was not found. Run install.cmd instead of update.cmd."
}

$passwd = & wsl.exe -d $DistroName -u root --cd / -- getent passwd 1000
if ($LASTEXITCODE -ne 0 -or -not $passwd) {
    throw "Could not determine the CoKernel Linux user (expected UID 1000)."
}
$linuxUser = (($passwd | Select-Object -First 1) -split ':')[0]

Write-Host "==> Applying CoKernel migrations, rebuilding services, and running smoke tests"
& wsl.exe -d $DistroName -u $linuxUser --cd / -- bash -lc "cd ~/src/CoKernel && ./scripts/update.sh"
if ($LASTEXITCODE -ne 0) {
    throw "CoKernel WSL update failed. Inspect the output above or run ./logs.sh inside the CoKernel WSL distro."
}

Write-Host "==> Verifying Windows localhost reachability"
& powershell.exe -NoProfile -ExecutionPolicy Bypass -File (Join-Path $PSScriptRoot "verify-windows-loopback.ps1") -DistroName $DistroName
if ($LASTEXITCODE -ne 0) {
    throw "CoKernel services are healthy inside WSL, but Windows localhost acceptance failed."
}

Write-Host ""
Write-Host "CoKernel update completed successfully." -ForegroundColor Green
Write-Host "JupyterLab: http://localhost:8888"
