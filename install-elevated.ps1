$ErrorActionPreference = 'Continue'
Set-StrictMode -Version Latest

$logDir = Join-Path $env:LOCALAPPDATA 'CoKernel'
$logPath = Join-Path $logDir 'install.log'
New-Item -ItemType Directory -Path $logDir -Force | Out-Null

Write-Host "CoKernel elevated installer wrapper"
Write-Host "Log: $logPath"
Write-Host ""

$installer = Join-Path $PSScriptRoot 'install.ps1'
if (-not (Test-Path -LiteralPath $installer)) {
    "install.ps1 was not found at $installer" | Tee-Object -FilePath $logPath -Append
    exit 1
}

"`n===== CoKernel install started $(Get-Date -Format o) =====" | Out-File -FilePath $logPath -Append -Encoding utf8

& powershell.exe -NoProfile -ExecutionPolicy Bypass -File $installer 2>&1 |
    Tee-Object -FilePath $logPath -Append

$code = $LASTEXITCODE
if ($null -eq $code) {
    $code = 1
}

"===== CoKernel install finished with exit code $code at $(Get-Date -Format o) =====`n" |
    Out-File -FilePath $logPath -Append -Encoding utf8

exit $code
