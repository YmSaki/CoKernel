[CmdletBinding()]
param(
    [string]$DistroName = "CoKernel",
    [string]$Distribution = "Ubuntu-24.04",
    [string]$LinuxUser = "",
    [string]$InstallLocation = "$env:LOCALAPPDATA\CoKernel\WSL",
    [switch]$RestartNow,
    [switch]$NoStart
)

$ErrorActionPreference = "Stop"
Set-StrictMode -Version Latest

function Write-Step {
    param([string]$Message)
    Write-Host ""
    Write-Host "==> $Message" -ForegroundColor Cyan
}

function Test-IsAdministrator {
    $identity = [Security.Principal.WindowsIdentity]::GetCurrent()
    $principal = New-Object Security.Principal.WindowsPrincipal($identity)
    return $principal.IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)
}

function Ensure-Elevated {
    if (Test-IsAdministrator) {
        return
    }

    Write-Host "CoKernel setup needs Administrator privileges. Opening UAC..."
    $arguments = @(
        "-NoProfile",
        "-ExecutionPolicy", "Bypass",
        "-File", ('"{0}"' -f $PSCommandPath)
    )
    Start-Process -FilePath "powershell.exe" -Verb RunAs -ArgumentList $arguments
    exit 0
}

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

function Test-WslDistroExists {
    param([string]$Name)
    return (Get-WslDistros) -contains $Name
}

function Invoke-Wsl {
    param(
        [Parameter(Mandatory = $true)][string[]]$Arguments,
        [switch]$AllowFailure
    )

    & wsl.exe @Arguments
    $code = $LASTEXITCODE
    if (-not $AllowFailure -and $code -ne 0) {
        throw "wsl.exe failed with exit code $code. Arguments: $($Arguments -join ' ')"
    }
    return $code
}

function Invoke-WslBash {
    param(
        [Parameter(Mandatory = $true)][string]$User,
        [Parameter(Mandatory = $true)][string]$Command
    )

    & wsl.exe -d $DistroName -u $User -- bash -lc $Command
    if ($LASTEXITCODE -ne 0) {
        throw "WSL command failed for user '$User': $Command"
    }
}

function Get-DefaultLinuxUser {
    $candidate = $env:USERNAME.ToLowerInvariant()
    $candidate = $candidate -replace "[^a-z0-9_-]", "-"
    $candidate = $candidate -replace "^[^a-z_]+", ""
    $candidate = $candidate -replace "-+", "-"
    $candidate = $candidate.Trim("-", "_")

    if ([string]::IsNullOrWhiteSpace($candidate)) {
        $candidate = "cokernel"
    }
    if ($candidate.Length -gt 24) {
        $candidate = $candidate.Substring(0, 24)
    }
    return $candidate
}

function Get-WindowsFeatureState {
    param([string]$Name)
    try {
        return (Get-WindowsOptionalFeature -Online -FeatureName $Name).State.ToString()
    }
    catch {
        return "Unknown"
    }
}

function Register-ResumeAfterLogon {
    $runOnce = "HKCU:\Software\Microsoft\Windows\CurrentVersion\RunOnce"
    $command = 'powershell.exe -NoProfile -ExecutionPolicy Bypass -File "{0}"' -f $PSCommandPath
    New-Item -Path $runOnce -Force | Out-Null
    New-ItemProperty -Path $runOnce -Name "CoKernelInstaller" -PropertyType String -Value $command -Force | Out-Null
}

function Ensure-WslPlatform {
    Write-Step "Checking WSL platform"

    $wslState = Get-WindowsFeatureState "Microsoft-Windows-Subsystem-Linux"
    $vmState = Get-WindowsFeatureState "VirtualMachinePlatform"

    if ($wslState -ne "Enabled" -or $vmState -ne "Enabled") {
        Write-Host "Enabling WSL and Virtual Machine Platform..."
        & wsl.exe --install --no-distribution
        if ($LASTEXITCODE -ne 0) {
            throw "wsl --install --no-distribution failed with exit code $LASTEXITCODE."
        }

        $wslState = Get-WindowsFeatureState "Microsoft-Windows-Subsystem-Linux"
        $vmState = Get-WindowsFeatureState "VirtualMachinePlatform"
        if ($wslState -ne "Enabled" -or $vmState -ne "Enabled") {
            Register-ResumeAfterLogon
            Write-Host ""
            Write-Host "Windows must restart before CoKernel setup can continue." -ForegroundColor Yellow
            Write-Host "The installer is registered to resume after your next sign-in."
            if ($RestartNow) {
                Restart-Computer -Force
            }
            else {
                Write-Host "Restart Windows, then accept the UAC prompt when CoKernel resumes."
            }
            exit 3010
        }
    }

    & wsl.exe --update
    if ($LASTEXITCODE -ne 0) {
        Write-Warning "wsl --update failed. Continuing with the installed WSL version."
    }

    $help = (& wsl.exe --help 2>&1 | Out-String)
    if ($help -notmatch "--name" -or $help -notmatch "--location") {
        throw "This WSL version is too old for CoKernel's named/location install. Run 'wsl --update', restart Windows, and run install.cmd again."
    }
}

function Ensure-CoKernelDistro {
    if (Test-WslDistroExists $DistroName) {
        Write-Step "Using existing WSL distribution '$DistroName'"
        return
    }

    Write-Step "Installing $Distribution as dedicated WSL2 distribution '$DistroName'"

    $parent = Split-Path -Parent $InstallLocation
    New-Item -ItemType Directory -Path $parent -Force | Out-Null

    if (Test-Path $InstallLocation) {
        $contents = @(Get-ChildItem -Force $InstallLocation -ErrorAction SilentlyContinue)
        if ($contents.Count -gt 0) {
            throw "Install location already contains files: $InstallLocation"
        }
    }

    $args = @(
        "--install", $Distribution,
        "--name", $DistroName,
        "--location", $InstallLocation,
        "--version", "2",
        "--no-launch"
    )

    & wsl.exe @args
    $code = $LASTEXITCODE
    if ($code -ne 0 -and -not (Test-WslDistroExists $DistroName)) {
        Write-Warning "Store-backed WSL install failed. Retrying with --web-download."
        & wsl.exe @args --web-download
        if ($LASTEXITCODE -ne 0 -and -not (Test-WslDistroExists $DistroName)) {
            throw "Failed to install $Distribution as $DistroName."
        }
    }

    if (-not (Test-WslDistroExists $DistroName)) {
        throw "WSL did not register distribution '$DistroName'."
    }
}

function Ensure-LinuxUser {
    Write-Step "Creating dedicated Linux user '$LinuxUser'"

    $command = @"
set -euo pipefail
export DEBIAN_FRONTEND=noninteractive
apt-get update
apt-get install -y sudo git ca-certificates
if ! id -u '$LinuxUser' >/dev/null 2>&1; then
  useradd --create-home --shell /bin/bash '$LinuxUser'
fi
usermod -aG sudo '$LinuxUser'
printf '%s ALL=(ALL) NOPASSWD:ALL\n' '$LinuxUser' > /etc/sudoers.d/90-cokernel-user
chmod 0440 /etc/sudoers.d/90-cokernel-user
mkdir -p /var/lib/cokernel
touch /etc/cokernel-managed
"@
    Invoke-WslBash -User "root" -Command $command
}

function Seed-Repository {
    Write-Step "Copying this CoKernel checkout into the WSL ext4 filesystem"

    $target = "/home/$LinuxUser/src/CoKernel"
    $seeded = & wsl.exe -d $DistroName -u root -- test -f /var/lib/cokernel/repo-seeded
    if ($LASTEXITCODE -eq 0) {
        Write-Host "Repository is already seeded at $target; preserving the WSL copy."
        return
    }

    $sourceWindows = (Resolve-Path $PSScriptRoot).Path
    $sourceLinux = (& wsl.exe -d $DistroName -u root -- wslpath -u $sourceWindows)
    if ($LASTEXITCODE -ne 0) {
        throw "Could not translate the Windows repository path into WSL."
    }
    $sourceLinux = ($sourceLinux | Select-Object -First 1).Trim()

    & wsl.exe -d $DistroName -u root -- mkdir -p "/home/$LinuxUser/src"
    if ($LASTEXITCODE -ne 0) { throw "Could not create WSL source directory." }

    & wsl.exe -d $DistroName -u root -- test -e $target
    if ($LASTEXITCODE -eq 0) {
        throw "Target already exists but was not marked as seeded: $target. Refusing to overwrite it."
    }

    & wsl.exe -d $DistroName -u root -- cp -a $sourceLinux $target
    if ($LASTEXITCODE -ne 0) { throw "Failed to copy the CoKernel repository into WSL." }

    & wsl.exe -d $DistroName -u root -- rm -f "$target/.env" "$target/.env.local"
    & wsl.exe -d $DistroName -u root -- chown -R "$LinuxUser`:$LinuxUser" $target
    & wsl.exe -d $DistroName -u root -- touch /var/lib/cokernel/repo-seeded

    $normalize = @"
set -euo pipefail
cd '$target'
find . -type f \( -name '*.sh' -o -name '*.py' -o -name '*.yaml' -o -name '*.yml' -o -name '*.Dockerfile' -o -name 'Dockerfile' \) -exec sed -i 's/\r$//' {} +
chmod +x up.sh down.sh logs.sh scripts/*.sh
"@
    Invoke-WslBash -User $LinuxUser -Command $normalize
}

function Configure-WslIsolation {
    Write-Step "Applying CoKernel WSL isolation settings"
    Invoke-WslBash -User $LinuxUser -Command "cd ~/src/CoKernel && ./scripts/harden-wsl.sh --yes"

    Write-Host "Restarting only the '$DistroName' WSL VM so systemd/isolation settings take effect..."
    & wsl.exe --terminate $DistroName
    Start-Sleep -Seconds 2
    & wsl.exe -d $DistroName -u $LinuxUser -- true
    if ($LASTEXITCODE -ne 0) {
        throw "Failed to restart the CoKernel WSL distribution."
    }
}

function Bootstrap-LinuxRuntime {
    Write-Step "Installing Docker Engine and NVIDIA Container Toolkit inside CoKernel WSL"
    Invoke-WslBash -User $LinuxUser -Command "cd ~/src/CoKernel && ./scripts/bootstrap-ubuntu.sh"

    Write-Host "Refreshing the WSL VM so Docker group membership is active..."
    & wsl.exe --terminate $DistroName
    Start-Sleep -Seconds 2
    & wsl.exe -d $DistroName -u $LinuxUser -- true
    if ($LASTEXITCODE -ne 0) {
        throw "Failed to restart the CoKernel WSL distribution after Docker setup."
    }
}

function Validate-And-Start {
    Write-Step "Creating local secrets and validating the installation"
    Invoke-WslBash -User $LinuxUser -Command "cd ~/src/CoKernel && ./scripts/init-env.sh"
    Invoke-WslBash -User $LinuxUser -Command "cd ~/src/CoKernel && ./scripts/doctor.sh"

    if (-not $NoStart) {
        Write-Step "Starting CoKernel"
        Invoke-WslBash -User $LinuxUser -Command "cd ~/src/CoKernel && ./up.sh"
    }
}

Ensure-Elevated

$os = Get-CimInstance Win32_OperatingSystem
$build = [int]$os.BuildNumber
if ($build -lt 19041) {
    throw "CoKernel requires Windows 10 build 19041+; Windows 11 is the primary target. Current build: $build"
}

if ([string]::IsNullOrWhiteSpace($LinuxUser)) {
    $LinuxUser = Get-DefaultLinuxUser
}
if ($LinuxUser -notmatch '^[a-z_][a-z0-9_-]*$') {
    throw "LinuxUser must match ^[a-z_][a-z0-9_-]*$. Value: $LinuxUser"
}

Write-Host "CoKernel Windows bootstrap" -ForegroundColor Green
Write-Host "  WSL distro:       $DistroName"
Write-Host "  Linux image:      $Distribution"
Write-Host "  Linux user:       $LinuxUser"
Write-Host "  Install location: $InstallLocation"
Write-Host "  Source checkout:  $PSScriptRoot"

try {
    Ensure-WslPlatform
    Ensure-CoKernelDistro
    Ensure-LinuxUser
    Seed-Repository
    Configure-WslIsolation
    Bootstrap-LinuxRuntime
    Validate-And-Start

    Write-Host ""
    Write-Host "CoKernel installation completed." -ForegroundColor Green
    Write-Host ""
    Write-Host "JupyterLab: http://localhost:8888"
    Write-Host "WSL shell:  wsl -d $DistroName"
    Write-Host "Repo:       /home/$LinuxUser/src/CoKernel"
    Write-Host ""
    Write-Host "The repository is private. For future 'git pull' inside WSL, configure GitHub authentication once inside this WSL distro (SSH key or GitHub CLI/PAT)."
    Write-Host "OpenAI tunnel credentials can be added later to ~/src/CoKernel/.env; Jupyter + MCP work locally without them."
}
catch {
    Write-Host ""
    Write-Host "CoKernel installation failed:" -ForegroundColor Red
    Write-Host $_.Exception.Message -ForegroundColor Red
    Write-Host ""
    Write-Host "The installer is idempotent and does not unregister existing distros. Fix the reported issue and run install.cmd again."
    exit 1
}
