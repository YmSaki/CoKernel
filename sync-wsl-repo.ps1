[CmdletBinding()]
param(
    [string]$DistroName = "CoKernel"
)

$ErrorActionPreference = "Stop"
Set-StrictMode -Version Latest

function Get-WslDistros {
    $output = & wsl.exe --list --quiet 2>$null
    if ($LASTEXITCODE -ne 0) {
        return @()
    }
    return @(
        $output |
            ForEach-Object { ($_ -replace "`0", "").Trim() } |
            Where-Object { $_ }
    )
}

if ((Get-WslDistros) -notcontains $DistroName) {
    exit 0
}

& wsl.exe -d $DistroName -u root --cd / -- test -f /var/lib/cokernel/repo-seeded
if ($LASTEXITCODE -ne 0) {
    exit 0
}

$passwd = & wsl.exe -d $DistroName -u root --cd / -- getent passwd 1000
if ($LASTEXITCODE -ne 0 -or -not $passwd) {
    throw "Could not determine the CoKernel Linux user (expected UID 1000)."
}
$linuxUser = (($passwd | Select-Object -First 1) -split ':')[0]
if ([string]::IsNullOrWhiteSpace($linuxUser)) {
    throw "Could not determine the CoKernel Linux username."
}

$sourceWindows = (Resolve-Path $PSScriptRoot).Path
if ($sourceWindows -notmatch '^([A-Za-z]):\\(.*)$') {
    throw "CoKernel Windows checkout must be on a local drive for WSL synchronization: $sourceWindows"
}

$driveUpper = $Matches[1].ToUpperInvariant()
$driveLower = $Matches[1].ToLowerInvariant()
$rest = $Matches[2] -replace '\\', '/'
$mountPoint = "/mnt/$driveLower"
$sourceLinux = "$mountPoint/$rest"
$target = "/home/$linuxUser/src/CoKernel"
$mountedTemporarily = $false

try {
    & wsl.exe -d $DistroName -u root --cd / -- test -d $sourceLinux
    if ($LASTEXITCODE -ne 0) {
        Write-Host "Temporarily mounting ${driveUpper}: inside $DistroName for installer synchronization..."
        & wsl.exe -d $DistroName -u root --cd / -- mkdir -p $mountPoint
        if ($LASTEXITCODE -ne 0) { throw "Could not create $mountPoint in WSL." }

        & wsl.exe -d $DistroName -u root --cd / -- mount -t drvfs "${driveUpper}:" $mountPoint
        if ($LASTEXITCODE -ne 0) { throw "Could not temporarily mount ${driveUpper}: at $mountPoint." }
        $mountedTemporarily = $true
    }

    & wsl.exe -d $DistroName -u root --cd / -- test -d $sourceLinux
    if ($LASTEXITCODE -ne 0) {
        throw "Windows checkout is not visible inside WSL at $sourceLinux."
    }

    & wsl.exe -d $DistroName -u root --cd / -- bash -lc "command -v rsync >/dev/null 2>&1 || (export DEBIAN_FRONTEND=noninteractive; apt-get update && apt-get install -y rsync)"
    if ($LASTEXITCODE -ne 0) {
        throw "Could not install or locate rsync inside $DistroName."
    }

    Write-Host "Synchronizing Windows checkout into WSL ext4: $target"
    & wsl.exe -d $DistroName -u root --cd / -- rsync -rt --delete --exclude=.env --exclude=.env.local --exclude=workspace/ "$sourceLinux/" "$target/"
    if ($LASTEXITCODE -ne 0) {
        throw "Repository synchronization into WSL failed."
    }

    & wsl.exe -d $DistroName -u root --cd / -- chown -R "${linuxUser}:${linuxUser}" $target
    if ($LASTEXITCODE -ne 0) {
        throw "Could not restore WSL repository ownership to $linuxUser."
    }

    & wsl.exe -d $DistroName -u $linuxUser --cd $target -- bash -lc "find . -type f \\( -name '*.sh' -o -name '*.py' -o -name '*.yaml' -o -name '*.yml' -o -name '*.Dockerfile' -o -name 'Dockerfile' \\) -exec sed -i 's/\\r$//' {} + && chmod +x up.sh down.sh logs.sh scripts/*.sh"
    if ($LASTEXITCODE -ne 0) {
        throw "Could not normalize synchronized repository files inside WSL."
    }

    Write-Host "CoKernel WSL checkout synchronized to the Windows checkout."
}
finally {
    if ($mountedTemporarily) {
        & wsl.exe -d $DistroName -u root --cd / -- umount $mountPoint
        if ($LASTEXITCODE -ne 0) {
            Write-Warning "Could not unmount temporary installer mount $mountPoint."
        }
    }
}
