# Run the production binary with no Host or development server. Inspect the
# rendered controls through Windows UI Automation, without enabling DevTools.
$ErrorActionPreference = 'Stop'
Add-Type -AssemblyName UIAutomationClient
Add-Type -AssemblyName UIAutomationTypes
if (Get-NetTCPConnection -State Listen -LocalPort 1420 -ErrorAction SilentlyContinue) {
    throw 'A development server is listening on the port used by candidate.8.'
}
$exe = (Resolve-Path 'target/release/cokernel-desktop.exe').Path
$app = Start-Process -FilePath $exe -PassThru
$children = [Windows.Automation.TreeScope]::Children
$descendants = [Windows.Automation.TreeScope]::Descendants
$owner = New-Object Windows.Automation.PropertyCondition([Windows.Automation.AutomationElement]::ProcessIdProperty,$app.Id)
$names = @()
try {
    $deadline = [datetime]::UtcNow.AddSeconds(80)
    do {
        if ($app.HasExited) { throw "Desktop exited with code $($app.ExitCode)." }
        $windows = [Windows.Automation.AutomationElement]::RootElement.FindAll($children,$owner)
        foreach ($window in $windows) {
            $elements = $window.FindAll($descendants,[Windows.Automation.Condition]::TrueCondition)
            $names = @($window.Current.Name) + @($elements | ForEach-Object { $_.Current.Name } | Where-Object { $_ })
            $buttons = @($elements | Where-Object { $_.Current.ControlType -eq [Windows.Automation.ControlType]::Button } | ForEach-Object { $_.Current.Name })
            if ($names -contains 'Projects' -and $buttons -contains 'APIキー・接続設定') {
                Write-Host 'The release exe rendered the Projects screen and API key button without a development server.'
                return
            }
        }
        Start-Sleep -Milliseconds 400
    } while ([datetime]::UtcNow -lt $deadline)
    throw "No rendered Projects screen. Desktop window/controls: $($names -join ' | ')"
} finally {
    if (-not $app.HasExited) { $app.Kill(); $app.WaitForExit() }
}
