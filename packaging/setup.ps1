[CmdletBinding()]
param([switch]$Resume)
$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest
$root = Join-Path $env:LOCALAPPDATA 'CoKernelV1'
$runtime = Join-Path $root 'runtime'
$config = Join-Path $runtime 'host-wsl.json'
$state = Join-Path $root 'host'
$hostExe = Join-Path $PSScriptRoot 'cokernel-host.exe'
$bootstrap = Join-Path $PSScriptRoot 'cokernel-bootstrap.exe'
$wsl = Join-Path $env:SystemRoot 'System32\wsl.exe'
$runOnce = 'HKCU:\Software\Microsoft\Windows\CurrentVersion\RunOnce'
New-Item -ItemType Directory -Force $root | Out-Null
Start-Transcript -Path (Join-Path $root 'setup.log') -Append | Out-Null
try {
    if ([Environment]::OSVersion.Version.Build -lt 22000) { throw 'CoKernel requires Windows 11.' }
    if ([IntPtr]::Size -ne 8) { throw 'CoKernel requires 64-bit Windows.' }
    # Microsoft documents these two locations for 64-bit Evergreen installations.
    $webviewKeys = @(
        'HKLM:\SOFTWARE\WOW6432Node\Microsoft\EdgeUpdate\Clients\{F3017226-FE2A-4295-8BDF-00C3A9A7E4C5}',
        'HKCU:\Software\Microsoft\EdgeUpdate\Clients\{F3017226-FE2A-4295-8BDF-00C3A9A7E4C5}'
    )
    $hasWebview = $false
    foreach ($key in $webviewKeys) {
        $version = Get-ItemPropertyValue -Path $key -Name pv -ErrorAction SilentlyContinue
        if ($version -and ([version]$version -gt [version]'0.0.0.0')) { $hasWebview = $true }
    }
    if (-not $hasWebview) {
        $webview = Join-Path $PSScriptRoot 'MicrosoftEdgeWebview2Setup.exe'
        $signature = Get-AuthenticodeSignature -LiteralPath $webview
        if ($signature.Status -ne 'Valid' -or $signature.SignerCertificate.Subject -notmatch 'O=Microsoft Corporation(,|$)') {
            throw 'WebView2 bootstrapper signature is invalid.'
        }
        $install = Start-Process -FilePath $webview -ArgumentList '/silent /install' -Wait -PassThru
        if ($install.ExitCode -ne 0) { throw 'WebView2 installation failed. Check internet access and retry setup.' }
    }
    & $wsl --status *> $null
    if ($LASTEXITCODE -ne 0) {
        # Only feature enablement is elevated. Import/Host/credentials stay with the original user.
        $resumeCommand = 'powershell.exe -NoProfile -ExecutionPolicy Bypass -File "{0}" -Resume' -f $PSCommandPath
        New-Item -Path $runOnce -Force | Out-Null
        New-ItemProperty -Path $runOnce -Name CoKernelV1Setup -Value $resumeCommand -PropertyType String -Force | Out-Null
        $enabled = Start-Process -FilePath $wsl -ArgumentList '--install --no-distribution' -Verb RunAs -Wait -PassThru
        if ($enabled.ExitCode -ne 0 -and $enabled.ExitCode -ne 3010) { throw 'WSL installation failed. See Windows virtualization settings and retry setup.' }
        Write-Host 'Restart Windows. CoKernel setup will resume after sign-in.'
        exit 3010
    }
    $existingHosts = @(Get-Process -Name cokernel-host -ErrorAction SilentlyContinue)
    foreach ($existingHost in $existingHosts) {
        if ($existingHost.Path -ne $hostExe) { throw 'A different CoKernel Host is running. Stop that runtime before installing this release.' }
    }
    $image = Join-Path $PSScriptRoot 'runtime.tar'
    $receipt = Get-Content -Raw -LiteralPath ($image + '.json') | ConvertFrom-Json
    if (Test-Path -LiteralPath $config) {
        # Never apply a different runtime over an existing installation silently.
        $installed = Get-Content -Raw -LiteralPath (Join-Path $runtime 'provisioned.json') | ConvertFrom-Json
        if ($installed.identity -ne $receipt.identity) { throw 'Installed runtime differs from this package. In-place updates require a verified migration; existing notebooks have been preserved.' }
    } else {
        if (Test-Path -LiteralPath $runtime) { throw 'An incomplete installation exists. Inspect setup.log before repair; it will not be overwritten or deleted.' }
        $drive = Get-PSDrive -Name ([IO.Path]::GetPathRoot($root).Substring(0,1))
        if ($drive.Free -lt 10GB) { throw 'At least 10 GB free disk space is required for setup.' }
        & $bootstrap import-validation $image $runtime
        if ($LASTEXITCODE -ne 0) { throw 'Managed runtime import failed. Existing distributions and data were preserved.' }
    }
    $wslConfig = Get-Content -Raw -LiteralPath $config | ConvertFrom-Json
    & $wsl -d $wslConfig.distribution -u root --cd / --exec /opt/cokernel/bin/cokernel-bootstrap inspect
    if ($LASTEXITCODE -ne 0) { throw 'Runtime integrity check failed.' }
    & $wsl -d $wslConfig.distribution -u cokernel-control --cd / --exec /usr/lib/wsl/lib/nvidia-smi -L
    if ($LASTEXITCODE -ne 0) { throw 'NVIDIA GPU is unavailable. Install the current Windows NVIDIA driver and retry CoKernel setup.' }
    & $hostExe check *> $null
    if ($LASTEXITCODE -ne 0 -and $existingHosts.Count -gt 0) {
        & $hostExe start
        if ($LASTEXITCODE -ne 0) { throw 'Existing Host refused runtime start.' }
    } elseif ($LASTEXITCODE -ne 0) {
        $arguments = 'serve "{0}" "{1}" --start --background' -f $config,$state
        Start-Process -FilePath $hostExe -ArgumentList $arguments -WindowStyle Hidden | Out-Null
    }
    $ready = $false
    for ($attempt = 0; $attempt -lt 60; $attempt++) {
        & $hostExe check *> $null
        if ($LASTEXITCODE -eq 0) { $ready = $true; break }
        Start-Sleep -Seconds 1
    }
    if (-not $ready) { throw 'Host did not report HEALTHY. Setup remains incomplete; see setup.log.' }
    & $hostExe autostart enable $config $state
    if ($LASTEXITCODE -ne 0) { throw 'Could not register startup. Setup remains incomplete.' }
    Remove-ItemProperty -Path $runOnce -Name CoKernelV1Setup -ErrorAction SilentlyContinue
    Start-Process -FilePath (Join-Path $PSScriptRoot 'cokernel-desktop.exe') | Out-Null
    Write-Host 'CoKernel runtime is ready. Configure Remote AI in the application.'
} catch {
    Write-Host $_.Exception.Message -ForegroundColor Red
    Read-Host 'Press Enter to close; run CoKernel Setup again to retry'
    exit 1
} finally { Stop-Transcript | Out-Null }
