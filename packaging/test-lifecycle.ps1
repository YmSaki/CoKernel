$ErrorActionPreference='Stop'
. "$PSScriptRoot/lifecycle.ps1"
function Assert($value, $message) { if (-not $value) { throw $message } }
function Reject($action, $message) { $rejected=$false; try { & $action } catch { $rejected=$true }; Assert $rejected $message }
$testRoot=Join-Path ([IO.Path]::GetTempPath()) ('ck-lifecycle-'+[guid]::NewGuid())
New-Item -ItemType Directory $testRoot | Out-Null
$originalLocal=$env:LOCALAPPDATA
try {
    $env:LOCALAPPDATA=$testRoot
    Assert ((Get-ProductRoot) -eq (Join-Path $testRoot 'CoKernel')) 'Fresh installs must use stable product name.'
    $legacy=Join-Path $testRoot 'CoKernelV1'
    New-Item -ItemType Directory $legacy | Out-Null
    Assert ((Get-ProductRoot) -eq $legacy) 'Candidate.7 must retain credential-bound state directory.'
    Assert ((Get-PrepareTransition $legacy) -eq 'Fresh') 'Empty root must not prompt.'
    New-Item -ItemType File (Join-Path $legacy 'setup.log') | Out-Null
    Assert ((Get-PrepareTransition $legacy) -eq 'Fresh') 'A transcript is not compute data.'
    New-Item -ItemType Directory (Join-Path $legacy 'runtime') | Out-Null
    Reject { Get-PrepareTransition $legacy } 'Partial import must not be classified as fresh.'
    Remove-Item (Join-Path $legacy 'runtime')
    if ($env:OS -eq 'Windows_NT') {
        $scriptPath = Join-Path $PSScriptRoot 'setup.ps1'
        $testApp = Join-Path $testRoot 'app'
        & powershell.exe -NoProfile -File $scriptPath -Mode Prepare -AppRoot $testApp
        Assert ($LASTEXITCODE -eq 0) 'Fresh Prepare must proceed without maintenance consent.'
        New-Item -ItemType Directory (Join-Path $legacy 'runtime') | Out-Null
        & powershell.exe -NoProfile -File $scriptPath -Mode Prepare -AppRoot $testApp
        Assert ($LASTEXITCODE -eq 1) 'Incomplete Prepare must block instead of requesting consent.'
        Remove-Item (Join-Path $legacy 'runtime')
    }
    $modern=Join-Path $testRoot 'CoKernel'
    New-Item -ItemType Directory $modern | Out-Null
    Reject { Get-ProductRoot } 'Ambiguous installations must fail closed.'
    Remove-Item $modern
    $record=Join-Path $legacy 'record.json'
    Write-Record $record @{phase='old'}
    Write-Record $record @{phase='new'}
    Assert ((Read-Record $record).phase -eq 'new') 'Atomic record replacement failed.'
    [IO.File]::WriteAllText($record, ('x'*65537))
    Reject { Read-Record $record } 'Oversize state was accepted.'
    $runtime=Join-Path $legacy 'runtime'
    New-Item -ItemType Directory (Join-Path $runtime 'distro') -Force | Out-Null
    $identity=[guid]::NewGuid()
    $distro='CoKernelV1-Validation-'+$identity.ToString('N')
    Write-Record (Join-Path $runtime 'prepared.json') @{identity=$identity.ToString()}
    Write-Record (Join-Path $runtime 'provisioned.json') @{identity=$identity.ToString()}
    Write-Record (Join-Path $runtime 'host-wsl.json') @{distribution=$distro;linux_user='cokernel-control';runtime_path='/opt/cokernel/launch'}
    # Registry is deliberately modeled: never touch the runner's actual WSL registrations.
    $script:registration=[pscustomobject]@{DistributionName=$distro;BasePath=(Join-Path $runtime 'distro')}
    function Get-ChildItem { param($Path) if ($Path -like 'HKCU:*') { [pscustomobject]@{PSPath='fixture'} } else { Microsoft.PowerShell.Management\Get-ChildItem $Path } }
    function Get-ItemProperty { param($Path) $script:registration }
    Assert ((Get-PrepareTransition $legacy) -eq 'Maintain') 'Owned retained runtime needs consent.'
    Write-Record (Join-Path $legacy 'maintenance.json') @{schema=1}
    Assert ((Get-PrepareTransition $legacy) -eq 'Recover') 'Pending recovery needs consent.'
    Remove-Item (Join-Path $legacy 'maintenance.json')
    $owned=Get-OwnedRuntime $legacy
    Assert ($owned.Identity -eq $identity.ToString()) 'Owned candidate.7 was rejected.'
    $script:registration.BasePath=Join-Path $testRoot 'unrelated-distro'
    Reject { Get-OwnedRuntime $legacy } 'Foreign distro registration was accepted.'
    $script:registration.BasePath=Join-Path $runtime 'distro'
    Write-Record (Join-Path $runtime 'host-wsl.json') @{distribution='Ubuntu';linux_user='cokernel-control';runtime_path='/opt/cokernel/launch'}
    Reject { Get-OwnedRuntime $legacy } 'Arbitrary user distro name was accepted.'
    Remove-Item (Join-Path $runtime 'host-wsl.json')
    Remove-Item (Join-Path $runtime 'provisioned.json')
    Assert ((Get-OwnedRuntime $legacy).Config.distribution -eq $distro) 'Interrupted import could not be identified for explicit removal.'
    $script:registration.DistributionName='CoKernel'
    Assert ((Get-OwnedRuntime $legacy).Config.distribution -eq 'CoKernel') 'Fresh product import under a retained legacy root was misidentified.'
    $script:registration.DistributionName=$distro
    $script:registration=[pscustomobject]@{DistributionName='Other';BasePath=(Join-Path $runtime 'distro')}
    Reject { Get-OwnedRuntime $legacy -AllowMissing } 'A foreign distro at the product path was accepted for removal.'
    $script:registration.BasePath=Join-Path $testRoot 'unrelated-distro'
    Reject { Get-OwnedRuntime $legacy } 'Missing registration was accepted for update.'
    Assert (-not (Get-OwnedRuntime $legacy -AllowMissing).Registered) 'Interrupted full removal could not be resumed.'
    # Execute the production recovery functions with fake process/registry boundaries.
    # All journal and intent writes below use the real filesystem implementation.
    $tokens=$null; $parseErrors=$null
    $ast=[System.Management.Automation.Language.Parser]::ParseFile((Join-Path $PSScriptRoot 'setup.ps1'),[ref]$tokens,[ref]$parseErrors)
    Assert ($parseErrors.Count -eq 0) 'Setup parser errors.'
    foreach ($functionAst in $ast.FindAll({param($node) $node -is [System.Management.Automation.Language.FunctionDefinitionAst]}, $false)) {
        . ([scriptblock]::Create($functionAst.Extent.Text))
    }
    $root=$legacy; $state=Join-Path $root 'host'; $journal=Join-Path $root 'maintenance.json'
    $installedPath=Join-Path $root 'installed.json'; $AppRoot=Join-Path $testRoot 'app'; $hostExe='fixture-host'; $wsl='fixture-wsl'; $run='fixture-registry'
    New-Item -ItemType Directory $AppRoot,$state | Out-Null
    $config=Join-Path $runtime 'host-wsl.json'
    $script:registration=[pscustomobject]@{DistributionName=$distro;BasePath=(Join-Path $runtime 'distro')}
    Write-Record $config @{distribution=$distro;linux_user='cokernel-control';runtime_path='/opt/cokernel/launch'}
    function Stop-Product { param($AppRoot,$ControlExe) }
    function Invoke-Native { param($Exe,$Arguments) }
    function Remove-ItemProperty { param($Path,$Name) }
    $script:failRecovery=$true; $script:recoveryActions=@()
    function Invoke-Maintenance {
        param($Distro,$Action,$Identity)
        $script:recoveryActions += $Action
        if ($script:failRecovery) { throw 'Injected recovery interruption' }
    }
    $newIdentity=[guid]::NewGuid().ToString()
    Write-Record (Join-Path $runtime 'provisioned.json') @{identity=$newIdentity;phase='provisioned'}
    Assert ((Get-OwnedRuntime $root).Identity -eq $newIdentity) 'Updated image identity must not change registered distro ownership.'
    $recoveryRecord=@{ schema=1; identity=$identity.ToString(); staged=$true; committed=$false; installed=$null; desired='{"schema":1,"desired":"RUNNING"}'; 'remote-access'='{"schema":1,"enabled":true}'; autostart=$null }
    Write-Record $journal $recoveryRecord
    Reject { Recover-Update } 'Injected recovery failure was hidden.'
    Assert (Test-Path $journal) 'Failed recovery discarded its journal.'
    $script:failRecovery=$false
    Recover-Update
    Assert (-not (Test-Path $journal)) 'Successful recovery left an active journal.'
    Assert ((Read-Record (Join-Path $runtime 'provisioned.json')).identity -eq $identity.ToString()) 'Rollback did not restore image identity.'
    Assert ((Read-Record (Join-Path $state 'desired.json')).desired -eq 'RUNNING') 'Runtime intent was lost.'
    Assert ((Read-Record (Join-Path $state 'remote-access.json')).enabled) 'Remote intent was lost.'
    Write-Record (Join-Path $runtime 'provisioned.json') @{identity=$newIdentity;phase='provisioned'}
    $recoveryRecord.committed=$true
    Write-Record $journal $recoveryRecord
    Recover-Update
    Assert ($script:recoveryActions[-1] -eq 'commit') 'Committed update was rolled back.'
    Assert ((Read-Record (Join-Path $runtime 'provisioned.json')).identity -eq $newIdentity) 'Committed identity was reverted.'
    Write-Host 'Lifecycle ownership, retained-data discovery and journal tests passed.'
} finally { $env:LOCALAPPDATA=$originalLocal; Remove-Item $testRoot -Recurse -Force }
