[CmdletBinding()]
param(
    [ValidateSet('Install','Prepare','Remove','Preserve')][string]$Mode = 'Install',
    [string]$AppRoot = '',
    [switch]$Resume,
    [switch]$MaintenanceAccepted
)
. (Join-Path $PSScriptRoot 'lifecycle.ps1')
if (-not $AppRoot) { $AppRoot = Split-Path (Split-Path $PSScriptRoot -Parent) -Parent }
$root = Get-ProductRoot
Assert-PlainPath $root
Assert-PlainPath $AppRoot
$runtime = Join-Path $root 'runtime'
$config = Join-Path $runtime 'host-wsl.json'
$state = Join-Path $root 'host'
$journal = Join-Path $root 'maintenance.json'
$installedPath = Join-Path $root 'installed.json'
$hostExe = Join-Path $PSScriptRoot 'cokernel-host.exe'
$bootstrap = Join-Path $PSScriptRoot 'cokernel-bootstrap.exe'
$wsl = Join-Path $env:SystemRoot 'System32\wsl.exe'
$runOnce = 'HKCU:\Software\Microsoft\Windows\CurrentVersion\RunOnce'
$run = 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Run'
function Read-Optional([string]$Path) {
    if (Test-Path $Path) { return [IO.File]::ReadAllText($Path) }
    return $null
}
function Restore-Intent($record) {
    New-Item -ItemType Directory -Force $state | Out-Null
    foreach ($name in @('desired','remote-access')) {
        $path = Join-Path $state "$name.json"
        $value = $record.$name
        if ($null -eq $value) { Remove-Item -LiteralPath $path -Force -ErrorAction SilentlyContinue }
        else { [IO.File]::WriteAllText($path, $value, (New-Object Text.UTF8Encoding($false))) }
    }
}
function Start-Host([string]$Exe, [switch]$ForceStart) {
    $arguments = 'serve "{0}" "{1}" --background' -f $config,$state
    if ($ForceStart) { $arguments += ' --start' }
    Start-Process -FilePath $Exe -ArgumentList $arguments -WindowStyle Hidden | Out-Null
}
function Recover-Update {
    if (-not (Test-Path $journal)) { return }
    $record = Read-Record $journal
    $owned = Get-OwnedRuntime $root
    Stop-Product $AppRoot $hostExe
    Invoke-Native $wsl @('--terminate',$owned.Config.distribution)
    if ($record.committed) {
        Invoke-Maintenance $owned.Config.distribution 'commit' $owned.Identity
    } else {
        if ($record.staged) { Invoke-Maintenance $owned.Config.distribution 'rollback' $record.identity }
        Write-Record (Join-Path $runtime 'provisioned.json') @{ identity=$record.identity; phase='provisioned' }
        if ($record.installed) { [IO.File]::WriteAllText($installedPath, $record.installed, (New-Object Text.UTF8Encoding($false))) }
        else { Remove-Item -LiteralPath $installedPath -Force -ErrorAction SilentlyContinue }
    }
    Restore-Intent $record
    if ($record.committed -and $record.autostart) {
        $currentInstall = Read-Record $installedPath
        $currentHost = Join-Path $currentInstall.app_directory 'cokernel-host.exe'
        Assert-PlainPath $currentHost
        if (-not $currentHost.StartsWith($AppRoot.TrimEnd('\') + '\', [StringComparison]::OrdinalIgnoreCase)) { throw 'Invalid committed application path.' }
        Invoke-Native $currentHost @('autostart','enable',$config,$state)
    }
    elseif ($record.autostart) { New-ItemProperty -Path $run -Name CoKernelHost -Value $record.autostart -PropertyType String -Force | Out-Null }
    else { Remove-ItemProperty -Path $run -Name CoKernelHost -ErrorAction SilentlyContinue }
    if (-not $record.committed) {
        $oldDirectory = $AppRoot
        if ($record.installed) { $oldDirectory = ($record.installed | ConvertFrom-Json).app_directory }
        Assert-PlainPath $oldDirectory
        if ($oldDirectory -ne $AppRoot -and -not $oldDirectory.StartsWith($AppRoot.TrimEnd('\') + '\', [StringComparison]::OrdinalIgnoreCase)) { throw 'Invalid previous application directory.' }
        $oldHost = Join-Path $oldDirectory 'cokernel-host.exe'
        if (Test-Path $oldHost) {
            $shell = New-Object -ComObject WScript.Shell
            $link = $shell.CreateShortcut((Join-Path ([Environment]::GetFolderPath('Programs')) 'CoKernel.lnk'))
            $link.TargetPath = Join-Path $oldDirectory 'cokernel-desktop.exe'
            $link.Save()
        }
    }
    Remove-Item -LiteralPath $journal -Force
}
New-Item -ItemType Directory -Force $root | Out-Null
$lock = $null
try { $lock = [IO.File]::Open((Join-Path $root 'maintenance.lock'), 'OpenOrCreate', 'ReadWrite', 'None') }
catch { throw 'Another CoKernel maintenance operation is running.' }
Start-Transcript -Path (Join-Path $root 'setup.log') -Append | Out-Null
$removeRoot = $false
try {
    if ($Mode -in @('Prepare','Remove','Preserve')) {
        if (-not $MaintenanceAccepted) { throw 'Maintenance requires explicit acceptance of Session termination.' }
        if (Test-Path $journal) { Recover-Update }
        if ($Mode -eq 'Remove') { Assert-PlainTree $root }
        if ((Test-Path $config) -or ($Mode -eq 'Remove' -and (Test-Path (Join-Path $runtime 'prepared.json')))) {
            $owned = Get-OwnedRuntime $root -AllowMissing:($Mode -eq 'Remove')
            if ($Mode -eq 'Prepare') {
                $record = @{
                    schema=1; identity=$owned.Identity; staged=$false; committed=$false
                    installed=(Read-Optional $installedPath)
                    desired=(Read-Optional (Join-Path $state 'desired.json'))
                    'remote-access'=(Read-Optional (Join-Path $state 'remote-access.json'))
                    autostart=(Get-ItemPropertyValue -Path $run -Name CoKernelHost -ErrorAction SilentlyContinue)
                }
                Write-Record $journal $record
            }
            $preservedIntent = @{ desired=(Read-Optional (Join-Path $state 'desired.json')); 'remote-access'=(Read-Optional (Join-Path $state 'remote-access.json')) }
            Stop-Product $AppRoot $hostExe
            if ($Mode -eq 'Preserve') { Restore-Intent $preservedIntent }
            Remove-ItemProperty -Path $run -Name CoKernelHost -ErrorAction SilentlyContinue
            if ($owned.Registered) { Invoke-Native $wsl @('--terminate',$owned.Config.distribution) }
            if ($Mode -eq 'Remove') {
                if (Test-Path $state) { Invoke-Native $hostExe @('clear-credentials',$state) }
                if ($owned.Registered) { Invoke-Native $wsl @('--unregister',$owned.Config.distribution) }
                $removeRoot = $true
            }
        } elseif (Test-Path $runtime) {
            throw 'CK-SETUP-INCOMPLETE: Runtime import is incomplete; retained evidence requires repair before removal.'
        } else {
            Stop-Product $AppRoot $hostExe
            if ($Mode -eq 'Remove') { $removeRoot = $true }
        }
        if ($Mode -ne 'Prepare') {
            Remove-ItemProperty -Path $run -Name CoKernelHost -ErrorAction SilentlyContinue
            foreach ($name in @('CoKernelSetup','CoKernelV1Setup')) { Remove-ItemProperty -Path $runOnce -Name $name -ErrorAction SilentlyContinue }
        }
    } else {
        if ([Environment]::OSVersion.Version.Build -lt 22000 -or [IntPtr]::Size -ne 8) { throw 'CoKernel requires Windows 11 x64.' }
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
        New-ItemProperty -Path $runOnce -Name CoKernelSetup -Value $resumeCommand -PropertyType String -Force | Out-Null
        $enabled = Start-Process -FilePath $wsl -ArgumentList '--install --no-distribution' -Verb RunAs -Wait -PassThru
        if ($enabled.ExitCode -ne 0 -and $enabled.ExitCode -ne 3010) { throw 'WSL installation failed. See Windows virtualization settings and retry setup.' }
        Write-Host 'Restart Windows. CoKernel setup will resume after sign-in.'
        exit 3010
    }
        $image = Join-Path $PSScriptRoot 'runtime.tar'
        $receipt = Read-Record ($image + '.json')
        if ($receipt.base_sha256 -ne 'e77b6f10c2590cef872b33ee9f635a0e3fd1f57fb074c0e52b5c7f56147a0c86') { throw 'Unsupported runtime base; an explicit migration is required.' }
        if ($receipt.schema -ne 1 -or $receipt.image_sha256 -ne (Get-FileHash -LiteralPath $image -Algorithm SHA256).Hash.ToLowerInvariant()) {
            throw 'CK-SETUP-HASH: Runtime image verification failed.'
        }
        if (Test-Path $config) {
            $owned = Get-OwnedRuntime $root
            if (-not (Test-Path $journal)) { throw 'Run CoKernelSetup.exe to authorize maintenance before updating an installed runtime.' }
            $record = Read-Record $journal
            # Remote AI cannot mutate data during post-update validation.
            New-Item -ItemType Directory -Force $state | Out-Null
            Write-Record (Join-Path $state 'remote-access.json') @{ schema=1; enabled=$false }
            if ($owned.Identity -ne $receipt.identity) {
                if ($record.staged) { throw 'An interrupted update must be recovered by running the installer again.' }
                $record.staged = $true
                Write-Record $journal $record
                Invoke-Maintenance $owned.Config.distribution 'prepare' $owned.Identity
                Send-WslInput $owned.Config.distribution @('/bin/tar','-xpf','-','-C','/opt/.cokernel-maintenance/new','opt/cokernel','etc/cokernel-validation.json','etc/ssl/certs/ca-certificates.crt','etc/ssl/cert.pem') $image
                Invoke-Maintenance $owned.Config.distribution 'apply' ([string]$receipt.identity)
            }
        } else {
            if (Test-Path $runtime) { throw 'CK-SETUP-INCOMPLETE: Retained incomplete import needs repair.' }
            $drive = Get-PSDrive -Name ([IO.Path]::GetPathRoot($root).Substring(0,1))
            if ($drive.Free -lt 10GB) { throw 'At least 10 GB free disk space is required.' }
            Invoke-Native $bootstrap @('install',$image,$runtime)
            $owned = Get-OwnedRuntime $root
        }
        Invoke-Native $wsl @('-d',$owned.Config.distribution,'-u','root','--exec','/opt/cokernel/bin/cokernel-bootstrap','inspect')
        Invoke-Native $wsl @('-d',$owned.Config.distribution,'-u','cokernel-control','--exec','/usr/lib/wsl/lib/nvidia-smi','-L')
        Start-Host $hostExe -ForceStart
        $ready = $false
        for ($attempt=0; $attempt -lt 60; $attempt++) {
            & $hostExe check *> $null
            if ($LASTEXITCODE -eq 0) { $ready=$true; break }
            Start-Sleep -Seconds 1
        }
        if (-not $ready) { throw 'CK-SETUP-HEALTH: Runtime did not become healthy.' }
        if (Test-Path $journal) {
            $record = Read-Record $journal
            Stop-Product $AppRoot $hostExe
            Write-Record (Join-Path $runtime 'provisioned.json') @{ identity=$receipt.identity; phase='provisioned' }
            Restore-Intent $record
            if ($record.autostart) { Invoke-Native $hostExe @('autostart','enable',$config,$state) }
            Write-Record $installedPath @{ schema=1; version='1.0.0-alpha.0-candidate.8'; image_identity=$receipt.identity; app_directory=$PSScriptRoot }
            $record.committed = $true
            Write-Record $journal $record
            if ($record.staged) { Invoke-Maintenance $owned.Config.distribution 'commit' ([string]$receipt.identity) }
            Remove-Item -LiteralPath $journal -Force
            Start-Host $hostExe
        } else {
            Invoke-Native $hostExe @('autostart','enable',$config,$state)
        }
        Write-Record $installedPath @{ schema=1; version='1.0.0-alpha.0-candidate.8'; image_identity=$receipt.identity; app_directory=$PSScriptRoot }
        foreach ($name in @('CoKernelSetup','CoKernelV1Setup')) { Remove-ItemProperty -Path $runOnce -Name $name -ErrorAction SilentlyContinue }
        Start-Process -FilePath (Join-Path $PSScriptRoot 'cokernel-desktop.exe') | Out-Null
        Write-Host 'CoKernel setup completed.'
    }
} catch {
    Write-Host $_.Exception.Message -ForegroundColor Red
    if ($Mode -eq 'Install' -and (Test-Path $journal)) {
        try { Recover-Update; Write-Host 'Previous runtime restored. Run the installer to retry.' }
        catch { Write-Host "Recovery incomplete: $($_.Exception.Message). Keep setup.log and run the installer again." }
    }
    exit 1
} finally {
    Stop-Transcript | Out-Null
    $lock.Dispose()
    if ($removeRoot) { Remove-Item -LiteralPath $root -Recurse -Force }
}
