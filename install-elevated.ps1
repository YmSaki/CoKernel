$ErrorActionPreference = 'Stop'
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

function Get-CoKernelLinuxUser {
    $candidate = $env:USERNAME.ToLowerInvariant()
    $candidate = $candidate -replace '[^a-z0-9_-]', '-'
    $candidate = $candidate -replace '^[^a-z_]+', ''
    $candidate = $candidate -replace '-+', '-'
    $candidate = $candidate -replace '^[-_]+|[-_]+$', ''
    if ([string]::IsNullOrWhiteSpace($candidate)) {
        $candidate = 'cokernel'
    }
    if ($candidate.Length -gt 24) {
        $candidate = $candidate.Substring(0, 24)
    }
    return $candidate
}

function Write-CoKernelCloudInitSeed {
    $cloudInitDir = Join-Path $env:USERPROFILE '.cloud-init'
    $seedPath = Join-Path $cloudInitDir 'CoKernel.user-data'
    New-Item -ItemType Directory -Path $cloudInitDir -Force | Out-Null

    if (Test-Path -LiteralPath $seedPath) {
        $existing = Get-Content -LiteralPath $seedPath -Raw -ErrorAction SilentlyContinue
        if ($existing -notmatch '^# CoKernel managed cloud-init seed') {
            throw "Refusing to overwrite existing cloud-init seed: $seedPath"
        }
    }

    $linuxUser = Get-CoKernelLinuxUser
    $seed = @"
# CoKernel managed cloud-init seed
#cloud-config
users:
  - name: $linuxUser
    gecos: CoKernel user
    groups: [adm, cdrom, sudo, dip, plugdev]
    sudo: ALL=(ALL) NOPASSWD:ALL
    shell: /bin/bash
    lock_passwd: true
write_files:
  - path: /etc/wsl.conf
    append: true
    content: |
      [user]
      default=$linuxUser
"@

    Set-Content -LiteralPath $seedPath -Value $seed -Encoding utf8
    Write-Host "Prepared Ubuntu WSL cloud-init seed: $seedPath"
    Write-Host "First-boot user '$linuxUser' will be created non-interactively."
    return $seedPath
}

$seedPath = $null
try {
    $seedPath = Write-CoKernelCloudInitSeed

    "`n===== CoKernel install started $(Get-Date -Format o) =====" | Out-File -FilePath $logPath -Append -Encoding utf8

    $ErrorActionPreference = 'Continue'
    & powershell.exe -NoProfile -ExecutionPolicy Bypass -File $installer 2>&1 |
        Tee-Object -FilePath $logPath -Append

    $code = $LASTEXITCODE
    if ($null -eq $code) {
        $code = 1
    }

    "===== CoKernel install finished with exit code $code at $(Get-Date -Format o) =====`n" |
        Out-File -FilePath $logPath -Append -Encoding utf8

    if ($code -eq 0 -and $seedPath -and (Test-Path -LiteralPath $seedPath)) {
        Remove-Item -LiteralPath $seedPath -Force
        Write-Host "Removed consumed CoKernel cloud-init seed."
    }

    exit $code
}
catch {
    $message = "CoKernel bootstrap wrapper failed: $($_.Exception.Message)"
    $message | Tee-Object -FilePath $logPath -Append
    if ($seedPath) {
        Write-Host "Cloud-init seed retained for retry: $seedPath"
    }
    exit 1
}
