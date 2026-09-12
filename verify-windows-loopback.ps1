[CmdletBinding()]
param(
    [string]$DistroName = "CoKernel",
    [int]$TimeoutMilliseconds = 5000
)

$ErrorActionPreference = "Stop"
Set-StrictMode -Version Latest

function Test-TcpLoopbackPort {
    param(
        [Parameter(Mandatory = $true)][int]$Port,
        [Parameter(Mandatory = $true)][int]$Timeout
    )

    $client = New-Object System.Net.Sockets.TcpClient
    try {
        $async = $client.BeginConnect("127.0.0.1", $Port, $null, $null)
        if (-not $async.AsyncWaitHandle.WaitOne($Timeout, $false)) {
            return $false
        }
        $client.EndConnect($async)
        return $client.Connected
    }
    catch {
        return $false
    }
    finally {
        $client.Dispose()
    }
}

$metadata = & wsl.exe -d $DistroName -u root --cd / -- cat /var/lib/cokernel/loopback-proxy.env 2>$null
if ($LASTEXITCODE -ne 0 -or -not $metadata) {
    throw "CoKernel loopback proxy metadata was not found. Run install.cmd (repair) or update.cmd first."
}

$values = @{}
foreach ($line in $metadata) {
    $clean = ($line -replace "`0", "" -replace "`r", "").Trim()
    if ($clean -match '^([A-Z0-9_]+)=([0-9]+)$') {
        $values[$Matches[1]] = [int]$Matches[2]
    }
}

foreach ($key in @("JUPYTER_PORT", "MCP_PORT")) {
    if (-not $values.ContainsKey($key)) {
        throw "Missing $key in CoKernel loopback proxy metadata."
    }
}

$checks = @(
    @{ Name = "JupyterLab"; Port = $values["JUPYTER_PORT"] },
    @{ Name = "MCP"; Port = $values["MCP_PORT"] }
)

foreach ($check in $checks) {
    if (-not (Test-TcpLoopbackPort -Port $check.Port -Timeout $TimeoutMilliseconds)) {
        throw "$($check.Name) is healthy inside WSL but Windows cannot connect to 127.0.0.1:$($check.Port). The WSL localhost bridge is not working."
    }
    Write-Host "[OK] Windows localhost -> $($check.Name): 127.0.0.1:$($check.Port)"
}

Write-Host "Windows/WSL loopback acceptance check passed."
