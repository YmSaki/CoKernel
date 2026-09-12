[CmdletBinding()]
param(
    [ValidateSet("Start", "Stop", "Status")]
    [string]$Action = "Status",
    [string]$DistroName = "CoKernel"
)

$ErrorActionPreference = "Stop"
Set-StrictMode -Version Latest

$KeepaliveLock = "/tmp/cokernel-runtime.keepalive.lock"

function Get-WslDistros {
    param([switch]$RunningOnly)

    $args = @("--list", "--quiet")
    if ($RunningOnly) {
        $args = @("--list", "--running", "--quiet")
    }

    $output = & wsl.exe @args 2>$null
    if ($LASTEXITCODE -ne 0) {
        return @()
    }

    return @(
        $output |
            ForEach-Object { ($_ -replace "`0", "").Trim() } |
            Where-Object { $_ }
    )
}

function Test-DistroExists {
    return (Get-WslDistros) -contains $DistroName
}

function Test-DistroRunning {
    return (Get-WslDistros -RunningOnly) -contains $DistroName
}

function Assert-ManagedDistro {
    if (-not (Test-DistroExists)) {
        throw "WSL distribution '$DistroName' does not exist. Run install.cmd first."
    }

    & wsl.exe -d $DistroName -u root --cd / -- test -f /etc/cokernel-managed
    if ($LASTEXITCODE -ne 0) {
        throw "WSL distribution '$DistroName' is not marked as CoKernel-managed. Refusing to control its lifetime."
    }
}

function Get-CoKernelLinuxUser {
    $passwd = & wsl.exe -d $DistroName -u root --cd / -- getent passwd 1000
    if ($LASTEXITCODE -ne 0 -or -not $passwd) {
        throw "Could not determine the CoKernel Linux user (expected UID 1000)."
    }
    return (($passwd | Select-Object -First 1) -split ':')[0]
}

function Test-KeepaliveHeld {
    param([Parameter(Mandatory = $true)][string]$LinuxUser)

    $probeArgs = @(
        "-d", $DistroName,
        "-u", $LinuxUser,
        "--cd", "/",
        "--",
        "/usr/bin/flock", "-n", $KeepaliveLock,
        "/usr/bin/true"
    )
    & wsl.exe @probeArgs *> $null
    $code = $LASTEXITCODE

    if ($code -eq 0) {
        return $false
    }
    if ($code -eq 1) {
        return $true
    }
    throw "Could not inspect the CoKernel WSL keepalive lock (exit code $code)."
}

function Start-Keepalive {
    Assert-ManagedDistro
    $linuxUser = Get-CoKernelLinuxUser

    if (Test-KeepaliveHeld -LinuxUser $linuxUser) {
        Write-Host "CoKernel WSL runtime keeper is already active."
        return
    }

    # systemd services do not keep a WSL instance alive by themselves. Keep one
    # ordinary WSL client attached for as long as CoKernel should remain online.
    # flock makes this idempotent: concurrent start attempts leave only one holder.
    $argumentString = @(
        '-d', ('"{0}"' -f $DistroName),
        '-u', ('"{0}"' -f $linuxUser),
        '--cd', '/',
        '--',
        '/usr/bin/flock', '-n', $KeepaliveLock,
        '/usr/bin/sleep', 'infinity'
    ) -join ' '

    $process = Start-Process -FilePath "wsl.exe" -ArgumentList $argumentString -WindowStyle Hidden -PassThru
    Start-Sleep -Milliseconds 750

    if ($process.HasExited -and -not (Test-KeepaliveHeld -LinuxUser $linuxUser)) {
        throw "The CoKernel WSL runtime keeper exited immediately with code $($process.ExitCode)."
    }

    if (-not (Test-KeepaliveHeld -LinuxUser $linuxUser)) {
        throw "The CoKernel WSL runtime keeper did not acquire its lifetime lock."
    }

    Write-Host "CoKernel WSL runtime keeper started (Windows PID $($process.Id))."
}

function Stop-Runtime {
    if (-not (Test-DistroExists)) {
        Write-Host "CoKernel WSL distro is not installed."
        return
    }

    if (-not (Test-DistroRunning)) {
        Write-Host "CoKernel WSL runtime is already stopped."
        return
    }

    $linuxUser = Get-CoKernelLinuxUser

    Write-Host "Stopping CoKernel containers..."
    $stopArgs = @(
        "-d", $DistroName,
        "-u", $linuxUser,
        "--cd", "/",
        "--",
        "bash", "-lc", "cd ~/src/CoKernel && ./down.sh"
    )
    & wsl.exe @stopArgs
    if ($LASTEXITCODE -ne 0) {
        Write-Warning "CoKernel containers did not stop cleanly; terminating the dedicated WSL distro anyway."
    }

    Write-Host "Terminating dedicated WSL distro '$DistroName'..."
    & wsl.exe --terminate $DistroName
    if ($LASTEXITCODE -ne 0) {
        throw "Failed to terminate WSL distribution '$DistroName'."
    }

    Write-Host "CoKernel runtime stopped."
}

function Show-Status {
    if (-not (Test-DistroExists)) {
        Write-Host "CoKernel runtime: not installed"
        return
    }

    if (-not (Test-DistroRunning)) {
        Write-Host "CoKernel runtime: stopped"
        return
    }

    $linuxUser = Get-CoKernelLinuxUser
    if (Test-KeepaliveHeld -LinuxUser $linuxUser) {
        Write-Host "CoKernel runtime: running (persistent Windows WSL client attached)"
    }
    else {
        Write-Host "CoKernel runtime: running, but no persistent Windows WSL client is attached" -ForegroundColor Yellow
    }
}

switch ($Action) {
    "Start" { Start-Keepalive }
    "Stop" { Stop-Runtime }
    "Status" { Show-Status }
}
