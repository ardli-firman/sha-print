[CmdletBinding()]
param()

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest

$repoRoot = Split-Path -Parent $PSScriptRoot
$solution = Join-Path $repoRoot 'ShaPrint.sln'
$results = Join-Path $repoRoot 'TestResults'

Push-Location $repoRoot
try {
    dotnet restore $solution --disable-parallel -m:1 -p:UseSharedCompilation=false
    if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }

    dotnet build $solution -c Release --no-restore -m:1 -p:UseSharedCompilation=false
    if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }

    dotnet test 'ShaPrint.Tests/ShaPrint.Tests.csproj' -c Release --no-restore --no-build `
        --logger 'trx;LogFileName=ShaPrint.Tests.trx' `
        --results-directory $results
    if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
}
finally {
    Pop-Location
}
