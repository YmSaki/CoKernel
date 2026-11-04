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
    $script:registration=[pscustomobject]@{DistributionName='Other';BasePath=(Join-Path $runtime 'distro')}
    Reject { Get-OwnedRuntime $legacy } 'Missing registration was accepted for update.'
    Assert (-not (Get-OwnedRuntime $legacy -AllowMissing).Registered) 'Interrupted full removal could not be resumed.'
    Write-Host 'Lifecycle ownership, retained-data discovery and journal tests passed.'
} finally { $env:LOCALAPPDATA=$originalLocal; Remove-Item $testRoot -Recurse -Force }
