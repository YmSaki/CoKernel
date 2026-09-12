[CmdletBinding()]
param()

$ErrorActionPreference = "Stop"
Set-StrictMode -Version Latest

$wrapper = Join-Path $PSScriptRoot "install-elevated.ps1"
if (-not (Test-Path -LiteralPath $wrapper)) {
    throw "CoKernel installer wrapper was not found: $wrapper"
}

$quotedWrapper = '"{0}"' -f $wrapper
$arguments = "-NoProfile -ExecutionPolicy Bypass -File $quotedWrapper"

Write-Host "Requesting Administrator permission to provision the CoKernel WSL runtime..."
$process = Start-Process powershell.exe -Verb RunAs -PassThru -Wait -ArgumentList $arguments
if ($null -eq $process) {
    throw "Could not start the elevated CoKernel installer."
}

if ($process.ExitCode -ne 0) {
    throw "CoKernel runtime provisioning failed with exit code $($process.ExitCode). See %LOCALAPPDATA%\CoKernel\install.log."
}

Write-Host "CoKernel runtime provisioning completed successfully." -ForegroundColor Green
