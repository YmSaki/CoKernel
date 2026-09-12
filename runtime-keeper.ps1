[CmdletBinding()]
param(
    [Parameter(Mandatory = $true)][string]$DistroName,
    [Parameter(Mandatory = $true)][string]$LinuxUser,
    [Parameter(Mandatory = $true)][string]$KeepaliveLock,
    [Parameter(Mandatory = $true)][string]$LogPath
)

$ErrorActionPreference = "Continue"
Set-StrictMode -Version Latest

$logDir = Split-Path -Parent $LogPath
if ($logDir) {
    New-Item -ItemType Directory -Path $logDir -Force | Out-Null
}

function Write-KeeperLog {
    param([string]$Message)
    $timestamp = Get-Date -Format "yyyy-MM-ddTHH:mm:ss.fffK"
    Add-Content -LiteralPath $LogPath -Value "[$timestamp] $Message"
}

Write-KeeperLog "Starting CoKernel WSL runtime keeper for distro '$DistroName' as user '$LinuxUser'."

$wslArgs = @(
    "-d", $DistroName,
    "-u", $LinuxUser,
    "--cd", "/",
    "--",
    "/usr/bin/flock", "-n", $KeepaliveLock,
    "/usr/bin/tail", "-f", "/dev/null"
)

try {
    & wsl.exe @wslArgs *>> $LogPath
    $code = $LASTEXITCODE
}
catch {
    Write-KeeperLog "Failed to invoke wsl.exe: $($_.Exception.Message)"
    exit 1
}

Write-KeeperLog "wsl.exe keeper exited with code $code."
exit $code
