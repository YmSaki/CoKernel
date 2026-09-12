[CmdletBinding()]
param(
    [string]$Configuration = "Release",
    [string]$Runtime = "win-x64",
    [string]$Output = ""
)

$ErrorActionPreference = "Stop"
$repo = Split-Path -Parent (Split-Path -Parent $PSScriptRoot)
if (-not $Output) {
    $Output = Join-Path $repo "dist\desktop"
}

$project = Join-Path $PSScriptRoot "CoKernel.Desktop\CoKernel.Desktop.csproj"
New-Item -ItemType Directory -Path $Output -Force | Out-Null

dotnet publish $project `
    -c $Configuration `
    -r $Runtime `
    --self-contained true `
    -p:PublishSingleFile=true `
    -p:IncludeNativeLibrariesForSelfExtract=true `
    -o $Output

if ($LASTEXITCODE -ne 0) {
    throw "CoKernel Desktop publish failed."
}

Write-Host "Published CoKernel Desktop to $Output" -ForegroundColor Green
