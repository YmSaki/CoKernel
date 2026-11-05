# Shared by setup and uninstall. Never dot-source untrusted install state.
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
function Get-ProductRoot {
    $modern = Join-Path $env:LOCALAPPDATA 'CoKernel'
    $legacy = Join-Path $env:LOCALAPPDATA 'CoKernelV1'
    if ((Test-Path $modern) -and (Test-Path $legacy)) { throw 'CK-SETUP-AMBIGUOUS: Both CoKernel and CoKernelV1 data exist. No data was changed.' }
    if (Test-Path $legacy) { return $legacy }
    return $modern
}
function Assert-PlainPath([string]$Path) {
    $cursor = [IO.Path]::GetFullPath($Path)
    while ($cursor) {
        $item = Get-Item -LiteralPath $cursor -Force -ErrorAction SilentlyContinue
        if ($item -and ($item.Attributes -band [IO.FileAttributes]::ReparsePoint)) { throw 'CK-SETUP-PATH: Linked product paths are not supported.' }
        $cursor = Split-Path -Parent $cursor
    }
}
function Write-Record([string]$Path, $Value) {
    Assert-PlainPath $Path
    $temp = "$Path.new"
    Assert-PlainPath $temp
    $json = $Value | ConvertTo-Json -Depth 12
    $bytes = (New-Object Text.UTF8Encoding($false)).GetBytes($json)
    $file = [IO.File]::Open($temp, 'Create', 'Write', 'None')
    try { $file.Write($bytes, 0, $bytes.Length); $file.Flush($true) } finally { $file.Dispose() }
    if (Test-Path $Path) { [IO.File]::Replace($temp, $Path, [NullString]::Value) } else { [IO.File]::Move($temp, $Path) }
}
function Read-Record([string]$Path) {
    Assert-PlainPath $Path
    if ((Get-Item -LiteralPath $Path).Length -gt 65536) { throw 'CK-SETUP-LIMIT: State file too large.' }
    return Get-Content -LiteralPath $Path -Raw | ConvertFrom-Json
}
function Invoke-Native([string]$Exe, [string[]]$Arguments) {
    & $Exe @Arguments
    if ($LASTEXITCODE -ne 0) { throw "CK-SETUP-COMMAND: $([IO.Path]::GetFileName($Exe)) failed ($LASTEXITCODE)." }
}
function Get-OwnedRuntime([string]$Root, [switch]$AllowMissing) {
    Assert-PlainPath $Root
    $runtime = Join-Path $Root 'runtime'
    $configPath = Join-Path $runtime 'host-wsl.json'
    $receiptPath = Join-Path $runtime 'provisioned.json'
    if (-not (Test-Path $receiptPath)) { $receiptPath = Join-Path $runtime 'prepared.json' }
    $receipt = Read-Record $receiptPath
    $identity = ([guid]$receipt.identity).ToString()
    $original = Read-Record (Join-Path $runtime 'prepared.json')
    $allowed = @('CoKernel', "CoKernelV1-Validation-$(([guid]$original.identity).ToString('N'))")
    $registrations = @(Get-ChildItem 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Lxss' -ErrorAction SilentlyContinue | ForEach-Object { Get-ItemProperty $_.PSPath })
    $expected = [IO.Path]::GetFullPath((Join-Path $runtime 'distro')).TrimEnd('\')
    $atPath = @($registrations | Where-Object { ([string]$_.BasePath).Replace('\\?\','').TrimEnd('\') -ieq $expected })
    if (Test-Path $configPath) { $config = Read-Record $configPath }
    else {
        # A partial product install may use CoKernel even in a retained legacy
        # Windows root. Discover by registered path, never by directory spelling.
        $name = 'CoKernel'
        if ($atPath.Count -eq 1) { $name = $atPath[0].DistributionName }
        $config = [pscustomobject]@{ distribution=$name; linux_user='cokernel-control'; runtime_path='/opt/cokernel/launch' }
    }
    if ($config.distribution -notin $allowed -or $config.linux_user -ne 'cokernel-control' -or $config.runtime_path -ne '/opt/cokernel/launch') {
        throw 'CK-SETUP-OWNERSHIP: Unrecognized runtime configuration.'
    }
    $entries = @($registrations | Where-Object DistributionName -eq $config.distribution)
    if ($entries.Count -eq 0 -and $atPath.Count -eq 0 -and $AllowMissing) { return @{ Config=$config; Identity=$identity; ConfigPath=$configPath; Registered=$false } }
    if ($entries.Count -ne 1 -or $atPath.Count -ne 1 -or $atPath[0].DistributionName -ne $config.distribution -or ([string]$entries[0].BasePath).Replace('\\?\','').TrimEnd('\') -ine $expected) {
        throw 'CK-SETUP-OWNERSHIP: WSL registration does not point to this product directory.'
    }
    return @{ Config = $config; Identity = $identity; ConfigPath = $configPath; Registered=$true }
}
function Get-PrepareTransition([string]$Root) {
    $config = Join-Path $Root 'runtime/host-wsl.json'
    if (Test-Path -LiteralPath $config -PathType Leaf) {
        $null = Get-OwnedRuntime $Root
        if (Test-Path -LiteralPath (Join-Path $Root 'maintenance.json')) { return 'Recover' }
        return 'Maintain'
    }
    # A failed preflight may leave only its transcript and lock behind.
    $entries = @(Get-ChildItem -LiteralPath $Root -Force -ErrorAction Stop |
        Where-Object { $_.PSIsContainer -or $_.Name -notin @('setup.log','maintenance.lock') })
    if ($entries.Count) { throw 'CK-SETUP-INCOMPLETE: Unrecognized or incomplete CoKernel data. Inspect setup.log before repair.' }
    return 'Fresh'
}
function Stop-Product([string]$AppRoot, [string]$ControlExe) {
    Assert-PlainPath $AppRoot
    foreach ($name in @('cokernel-host','cokernel-desktop')) {
        foreach ($process in @(Get-Process -Name $name -ErrorAction SilentlyContinue)) {
            if (-not $process.Path -or -not $process.Path.StartsWith($AppRoot.TrimEnd('\') + '\', [StringComparison]::OrdinalIgnoreCase)) {
                throw 'CK-SETUP-PROCESS: Another CoKernel installation is running. Close it before maintenance.'
            }
        }
    }
    if (@(Get-Process -Name cokernel-host -ErrorAction SilentlyContinue).Count) {
        try { Invoke-Native $ControlExe @('stop') }
        catch { Write-Warning 'Graceful stop failed. Terminating only verified CoKernel processes as accepted in the maintenance dialog.' }
    }
    foreach ($name in @('cokernel-desktop','cokernel-host')) {
        foreach ($process in @(Get-Process -Name $name -ErrorAction SilentlyContinue)) {
            if (-not $process.Path -or -not $process.Path.StartsWith($AppRoot.TrimEnd('\') + '\', [StringComparison]::OrdinalIgnoreCase)) { throw 'Another installation started during maintenance.' }
            $process.Kill(); $process.WaitForExit()
        }
    }
}
function Send-WslInput([string]$Distro, [string[]]$Command, [string]$InputFile) {
    # .NET binary stream: PowerShell's text pipeline would corrupt the tar archive.
    $info = New-Object Diagnostics.ProcessStartInfo
    $info.FileName = Join-Path $env:SystemRoot 'System32\wsl.exe'
    $args = @('-d',$Distro,'-u','root','--cd','/','--exec') + $Command
    foreach ($arg in $args) { if ($arg.Contains('"') -or $arg.Contains("`n")) { throw 'Invalid maintenance argument.' } }
    $info.Arguments = ($args | ForEach-Object { '"' + $_ + '"' }) -join ' '
    $info.UseShellExecute = $false; $info.RedirectStandardInput = $true
    $process = [Diagnostics.Process]::Start($info)
    try {
        $inputStream = [IO.File]::OpenRead($InputFile)
        try { $inputStream.CopyTo($process.StandardInput.BaseStream) } finally { $inputStream.Dispose(); $process.StandardInput.Close() }
        $process.WaitForExit()
        if ($process.ExitCode) { throw "CK-SETUP-WSL: Maintenance failed ($($process.ExitCode))." }
    } finally { $process.Dispose() }
}
function Invoke-Maintenance([string]$Distro, [string]$Action, [string]$Identity) {
    Send-WslInput $Distro @('/bin/sh','-s','--',$Action,$Identity) (Join-Path $PSScriptRoot 'runtime-maintenance.sh')
}

function Assert-PlainTree([string]$Root) {
    Assert-PlainPath $Root
    $pending = New-Object 'Collections.Generic.Stack[string]'
    $pending.Push($Root)
    while ($pending.Count) {
        $directory = New-Object IO.DirectoryInfo($pending.Pop())
        foreach ($entry in $directory.EnumerateFileSystemInfos()) {
            if ($entry.Attributes -band [IO.FileAttributes]::ReparsePoint) { throw 'CK-SETUP-PATH: Remove linked entries before deleting product data.' }
            if ($entry.Attributes -band [IO.FileAttributes]::Directory) { $pending.Push($entry.FullName) }
        }
    }
}
