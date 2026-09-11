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
        "-File", ('\"{0}\"' -f $PSCommandPath)
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

function Invoke-WslBash {
    param(
        [Parameter(Mandatory = $true)][string]$User,
        [Parameter(Mandatory = $true)][string]$Command,
        [string]$Step = "running Linux command"
    )

    Write-Host "[WSL] $Step"

    # Windows PowerShell here-strings use CRLF. Passing them verbatim to
    # `bash -lc` makes Bash see option names such as `pipefail\r`.
    $normalizedCommand = $Command -replace "`r", ""

    & wsl.exe -d $DistroName -u $User --cd / -- bash -lc $normalizedCommand
    $code = $LASTEXITCODE
    if ($code -ne 0) {
        throw "WSL step '$Step' failed for user '$User' with exit code $code. See the diagnostic output immediately above and in install.log."
    }
}

function Convert-WindowsDrivePathToWsl {
    param([Parameter(Mandatory = $true)][string]$Path)

    $fullPath = [System.IO.Path]::GetFullPath($Path)
    if ($fullPath -notmatch '^([A-Za-z]):\\(.*)$') {
        throw "CoKernel source checkout must be on a local Windows drive for first-time seeding. Unsupported path: $fullPath"
    }

    $drive = $Matches[1].ToLowerInvariant()
    $rest = $Matches[2] -replace '\\', '/'
    return "/mnt/$drive/$rest"
}

function Get-DefaultLinuxUser {
    $candidate = $env:USERNAME.ToLowerInvariant()
    $candidate = $candidate -replace "[^a-z0-9_-]", "-"
    $candidate = $candidate -replace "^[^a-z_]+", ""
    $candidate = $candidate -replace "-+", "-"
    $candidate = $candidate -replace "^[-_]+|[-_]+$", ""

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

    & wsl.exe --status *> $null
    $wslReady = ($LASTEXITCODE -eq 0)

    if (-not $wslReady) {
        Write-Host "Enabling WSL and Virtual Machine Platform..."
        & wsl.exe --install --no-distribution
        if ($LASTEXITCODE -ne 0) {
            throw "wsl --install --no-distribution failed with exit code $LASTEXITCODE."
        }
    }

    $wslState = Get-WindowsFeatureState "Microsoft-Windows-Subsystem-Linux"
    $vmState = Get-WindowsFeatureState "VirtualMachinePlatform"
    if ($wslState -match "Pending" -or $vmState -match "Pending" -or $wslState -eq "Disabled" -or $vmState -eq "Disabled") {
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

    & wsl.exe --update
    if ($LASTEXITCODE -ne 0) {
        Write-Warning "wsl --update failed. Continuing with the installed WSL version."
    }

    Write-Host "WSL platform is ready; named/location support will be validated by the install command itself."
}

function Ensure-CoKernelDistro {
    if (Test-WslDistroExists $DistroName) {
        & wsl.exe -d $DistroName -u root --cd / -- test -f /etc/cokernel-managed
        if ($LASTEXITCODE -ne 0) {
            throw "A WSL distribution named '$DistroName' already exists but was not created by CoKernel. Refusing to modify it. Use another -DistroName or remove/rename that distro yourself."
        }
        Write-Step "Resuming existing CoKernel WSL distribution '$DistroName'"
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

    $installArgs = @(
        "--install", $Distribution,
        "--name", $DistroName,
        "--location", $InstallLocation,
        "--version", "2",
        "--no-launch"
    )

    & wsl.exe @installArgs
    $code = $LASTEXITCODE
    if ($code -ne 0 -and -not (Test-WslDistroExists $DistroName)) {
        Write-Warning "Store-backed WSL install failed. Retrying with --web-download."
        $webArgs = $installArgs + @("--web-download")
        & wsl.exe @webArgs
        if ($LASTEXITCODE -ne 0 -and -not (Test-WslDistroExists $DistroName)) {
            throw "Failed to install $Distribution as $DistroName using WSL named/location install. See the diagnostic log above for wsl.exe output."
        }
    }

    if (-not (Test-WslDistroExists $DistroName)) {
        throw "WSL did not register distribution '$DistroName'."
    }

    & wsl.exe -d $DistroName -u root --cd / -- bash -lc "mkdir -p /var/lib/cokernel && touch /etc/cokernel-managed"
    if ($LASTEXITCODE -ne 0) {
        throw "The CoKernel WSL distro was installed, but its management marker could not be created."
    }
}

function Ensure-LinuxUser {
    Write-Step "Preparing Ubuntu and dedicated Linux user '$LinuxUser'"

    Invoke-WslBash -User "root" -Step "initializing CoKernel management state" -Command @"
set -euo pipefail
mkdir -p /var/lib/cokernel
touch /etc/cokernel-managed
"@

    Invoke-WslBash -User "root" -Step "updating Ubuntu package metadata" -Command @"
set -euo pipefail
export DEBIAN_FRONTEND=noninteractive
apt-get update
"@

    Invoke-WslBash -User "root" -Step "installing base Ubuntu packages (sudo, git, ca-certificates)" -Command @"
set -euo pipefail
export DEBIAN_FRONTEND=noninteractive
apt-get install -y sudo git ca-certificates
"@

    Invoke-WslBash -User "root" -Step "creating Linux user '$LinuxUser'" -Command @"
set -euo pipefail
if ! id -u '$LinuxUser' >/dev/null 2>&1; then
  useradd --create-home --user-group --shell /bin/bash '$LinuxUser'
fi
"@

    Invoke-WslBash -User "root" -Step "adding Linux user '$LinuxUser' to sudo group" -Command @"
set -euo pipefail
usermod -aG sudo '$LinuxUser'
"@

    Invoke-WslBash -User "root" -Step "configuring bootstrap sudo policy for '$LinuxUser'" -Command @"
set -euo pipefail
printf '%s ALL=(ALL) NOPASSWD:ALL\n' '$LinuxUser' > /etc/sudoers.d/90-cokernel-user
chmod 0440 /etc/sudoers.d/90-cokernel-user
"@
}

function Seed-Repository {
    Write-Step "Copying this CoKernel checkout into the WSL ext4 filesystem"

    $target = "/home/$LinuxUser/src/CoKernel"
    & wsl.exe -d $DistroName -u root --cd / -- test -f /var/lib/cokernel/repo-seeded
    if ($LASTEXITCODE -eq 0) {
        Write-Host "Repository is already seeded at $target; preserving the WSL copy."
        return
    }

    $sourceWindows = (Resolve-Path $PSScriptRoot).Path
    $sourceLinux = Convert-WindowsDrivePathToWsl -Path $sourceWindows
    Write-Host "Source checkout inside WSL: $sourceLinux"

    # Avoid `wslpath` here. Windows PowerShell 5.1 can strip backslashes while
    # forwarding a Windows path through wsl.exe, turning C:\foo\bar into
    # C:foobar. Deterministic drive-letter conversion avoids that quoting layer.
    & wsl.exe -d $DistroName -u root --cd / -- test -d $sourceLinux
    if ($LASTEXITCODE -ne 0) {
        throw "Windows checkout is not visible at '$sourceLinux' inside WSL. Windows-drive automount must remain enabled until repository seeding completes."
    }

    & wsl.exe -d $DistroName -u root --cd / -- mkdir -p "/home/$LinuxUser/src"
    if ($LASTEXITCODE -ne 0) { throw "Could not create WSL source directory." }

    & wsl.exe -d $DistroName -u root --cd / -- test -e $target
    if ($LASTEXITCODE -eq 0) {
        throw "Target already exists but was not marked as seeded: $target. Refusing to overwrite it."
    }

    & wsl.exe -d $DistroName -u root --cd / -- cp -a $sourceLinux $target
    if ($LASTEXITCODE -ne 0) { throw "Failed to copy the CoKernel repository into WSL." }

    & wsl.exe -d $DistroName -u root --cd / -- rm -f "$target/.env" "$target/.env.local"
    & wsl.exe -d $DistroName -u root --cd / -- chown -R "$LinuxUser`:$LinuxUser" $target
    & wsl.exe -d $DistroName -u root --cd / -- touch /var/lib/cokernel/repo-seeded

    $normalize = @"
set -euo pipefail
cd '$target'
find . -type f \( -name '*.sh' -o -name '*.py' -o -name '*.yaml' -o -name '*.yml' -o -name '*.Dockerfile' -o -name 'Dockerfile' \) -exec sed -i 's/\r$//' {} +
chmod +x up.sh down.sh logs.sh scripts/*.sh
"@
    Invoke-WslBash -User $LinuxUser -Step "normalizing repository files inside WSL" -Command $normalize
}

function Configure-WslIsolation {
    Write-Step "Applying CoKernel WSL isolation settings"
    Invoke-WslBash -User $LinuxUser -Step "applying WSL isolation policy" -Command "cd ~/src/CoKernel && ./scripts/harden-wsl.sh --yes"

    Write-Host "Restarting only the '$DistroName' WSL VM so systemd/isolation settings take effect..."
    & wsl.exe --terminate $DistroName
    Start-Sleep -Seconds 2
    & wsl.exe -d $DistroName -u $LinuxUser --cd / -- true
    if ($LASTEXITCODE -ne 0) {
        throw "Failed to restart the CoKernel WSL distribution."
    }
}

function Bootstrap-LinuxRuntime {
    Write-Step "Installing Docker Engine and NVIDIA Container Toolkit inside CoKernel WSL"
    Invoke-WslBash -User $LinuxUser -Step "installing Docker/NVIDIA runtime" -Command "cd ~/src/CoKernel && ./scripts/bootstrap-ubuntu.sh"

    Write-Host "Refreshing the WSL VM so Docker group membership is active..."
    & wsl.exe --terminate $DistroName
    Start-Sleep -Seconds 2
    & wsl.exe -d $DistroName -u $LinuxUser --cd / -- true
    if ($LASTEXITCODE -ne 0) {
        throw "Failed to restart the CoKernel WSL distribution after Docker setup."
    }
}

function Validate-And-Start {
    Write-Step "Creating local secrets and validating the installation"
    Invoke-WslBash -User $LinuxUser -Step "creating CoKernel local secrets" -Command "cd ~/src/CoKernel && ./scripts/init-env.sh"
    Invoke-WslBash -User $LinuxUser -Step "running CoKernel doctor" -Command "cd ~/src/CoKernel && ./scripts/doctor.sh"

    if (-not $NoStart) {
        Write-Step "Starting CoKernel"
        Invoke-WslBash -User $LinuxUser -Step "starting CoKernel services" -Command "cd ~/src/CoKernel && ./up.sh"
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
    Write-Host "The installer is idempotent and never unregisters a WSL distro. Fix the reported issue and run install.cmd again."
    exit 1
}
