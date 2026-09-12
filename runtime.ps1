[CmdletBinding()]
param(
    [ValidateSet("Start", "Stop", "Status")]
    [string]$Action = "Status",
    [string]$DistroName = "CoKernel"
)

$ErrorActionPreference = "Stop"
Set-StrictMode -Version Latest

$KeepaliveLock = "/tmp/cokernel-runtime.keepalive.lock"
$KeeperScript = Join-Path $PSScriptRoot "runtime-keeper.ps1"
$KeeperLog = Join-Path $env:LOCALAPPDATA "CoKernel\runtime-keeper.log"

function Get-WslDistros {
    param([switch]$RunningOnly)

    $listArgs = @("--list", "--quiet")
    if ($RunningOnly) {
        $listArgs = @("--list", "--running", "--quiet")
    }

    $output = & wsl.exe @listArgs 2>$null
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

function Get-KeeperLogTail {
    if (-not (Test-Path -LiteralPath $KeeperLog)) {
        return "No runtime keeper log was created."
    }

    return ((Get-Content -LiteralPath $KeeperLog -Tail 30) -join [Environment]::NewLine)
}

function Start-Keepalive {
    Assert-ManagedDistro
    $linuxUser = Get-CoKernelLinuxUser

    if (Test-KeepaliveHeld -LinuxUser $linuxUser) {
        Write-Host "CoKernel WSL runtime keeper is already active."
        return
    }

    if (-not (Test-Path -LiteralPath $KeeperScript)) {
        throw "Missing runtime keeper script: $KeeperScript. Run update.cmd to synchronize the latest CoKernel files."
    }

    # Validate the Linux commands before detaching anything. This makes a
    # stripped-down Ubuntu image fail with a useful message rather than a
    # mysterious Windows child-process exit code.
    & wsl.exe -d $DistroName -u $linuxUser --cd / -- sh -lc "test -x /usr/bin/flock && test -x /usr/bin/tail"
    if ($LASTEXITCODE -ne 0) {
        throw "CoKernel runtime keeper requires /usr/bin/flock and /usr/bin/tail inside the managed WSL distro."
    }

    $keeperDir = Split-Path -Parent $KeeperLog
    New-Item -ItemType Directory -Path $keeperDir -Force | Out-Null
    Remove-Item -LiteralPath $KeeperLog -Force -ErrorAction SilentlyContinue

    # Launch a hidden Windows PowerShell process and let *that* process invoke
    # wsl.exe synchronously. Directly launching wsl.exe with Start-Process is
    # unreliable on some WSL builds because the relay process can exit before
    # the detached Linux command has established a durable Windows client.
    # Keeping the PowerShell host alive gives WSL an ordinary attached client.
    $runnerArgs = @(
        "-NoProfile",
        "-ExecutionPolicy", "Bypass",
        "-File", ('"{0}"' -f $KeeperScript),
        "-DistroName", ('"{0}"' -f $DistroName),
        "-LinuxUser", ('"{0}"' -f $linuxUser),
        "-KeepaliveLock", ('"{0}"' -f $KeepaliveLock),
        "-LogPath", ('"{0}"' -f $KeeperLog)
    ) -join ' '

    $runner = Start-Process -FilePath "powershell.exe" -ArgumentList $runnerArgs -WindowStyle Hidden -PassThru

    $deadline = (Get-Date).AddSeconds(8)
    do {
        Start-Sleep -Milliseconds 250

        if (Test-KeepaliveHeld -LinuxUser $linuxUser) {
            Write-Host "CoKernel WSL runtime keeper started (Windows PID $($runner.Id))."
            return
        }

        if ($runner.HasExited) {
            $tail = Get-KeeperLogTail
            throw "The CoKernel WSL runtime keeper host exited before acquiring the lifetime lock. Runner exit code: $($runner.ExitCode).`nRuntime keeper log:`n$tail"
        }
    } while ((Get-Date) -lt $deadline)

    $tail = Get-KeeperLogTail
    throw "The CoKernel WSL runtime keeper did not acquire its lifetime lock within 8 seconds.`nRuntime keeper log:`n$tail"
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
