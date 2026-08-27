#!/usr/bin/env pwsh
# Publish ShaPrint.UI (Avalonia) for Windows x64 as a self-contained single-file exe,
# zipped into artifacts/<version>/shaprint-win-x64-v<version>.zip.
#
# Usage:
#   ./scripts/publish-windows.ps1 [-Version <version>]
#
# -Version is optional. Default: `git describe --tags --always` (leading 'v' stripped,
# since NuGet/MSBuild reject 'v1.2.3' as a -p:Version), falling back to a DOTTED
# date+short-sha (NuGet requires Major.Minor...). The value is sanitized to
# [A-Za-z0-9._-] and validated as X.Y[.Z[.R]][-suffix] — the shape NuGet accepts.
# No secrets, no hardcoded paths, no admin rights required.

param(
    [string]$Version = ''
)

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest

function Resolve-Version {
    param([string]$V)
    if (-not $V) {
        $V = [string](git describe --tags --always 2>$null)
        if ($V) {
            $V = $V -replace '^v', ''
        }
        else {
            $V = "$(Get-Date -Format 'yyyy.MM.dd')-$(git rev-parse --short HEAD 2>$null)"
        }
    }
    if (-not $V) { $V = 'unknown' }
    # Sanitize: keep only letters/digits/'.'/'_'/'-' (defensive even for explicit input).
    $V = [regex]::Replace([string]$V, '[^A-Za-z0-9._-]', '-')
    if ($V -notmatch '^\d+(\.\d+){1,3}(-[A-Za-z0-9][A-Za-z0-9._-]*)?$') {
        throw "Version '$V' is not valid for -p:Version (expected X.Y[.Z[.R]][-suffix], e.g. 1.0.0-test)."
    }
    return $V
}

$Version = Resolve-Version -V $Version

$RepoRoot = (Resolve-Path (Join-Path $PSScriptRoot '..')).Path
$OutDir   = Join-Path $RepoRoot "artifacts\$Version"
$null = New-Item -ItemType Directory -Force -Path $OutDir

$Proj = Join-Path $RepoRoot 'ShaPrint.UI\ShaPrint.UI.csproj'
# Resolve a dotnet that actually has the .NET 8 SDK. The PATH entry may be a
# stripped host (e.g. an app-bundled dotnet without any SDK), so we verify with
# `dotnet --list-sdks` and fall back to the default install location.
function Get-DotNet {
    $candidates = @()
    $cmd = Get-Command dotnet -ErrorAction SilentlyContinue
    if ($cmd) { $candidates += $cmd.Source }
    $default = Join-Path $env:ProgramFiles 'dotnet\dotnet.exe'
    if (Test-Path $default) { $candidates += $default }
    foreach ($c in $candidates) {
        if (-not $c -or -not (Test-Path $c)) { continue }
        $sdks = & $c --list-sdks 2>$null
        if ($LASTEXITCODE -eq 0 -and ($sdks -match '8\.0\.')) {
            $env:DOTNET_ROOT = Split-Path $c -Parent
            return $c
        }
    }
    throw "No dotnet with the .NET 8 SDK found (checked PATH and '$default'). Install the .NET 8 SDK or run with it on PATH."
}

$DotNet = Get-DotNet

Write-Host "==> Publishing ShaPrint.UI for win-x64 (version: $Version)"
& $DotNet publish $Proj -f net8.0-windows10.0.17763 -c Release -r win-x64 --self-contained true `
    -p:PublishSingleFile=true -p:IncludeNativeLibrariesForSelfExtract=true -p:Version=$Version
if ($LASTEXITCODE -ne 0) { throw "dotnet publish failed with exit code $LASTEXITCODE" }

$PublishDir = Join-Path $RepoRoot 'ShaPrint.UI\bin\Release\net8.0-windows10.0.17763\win-x64\publish'
$Exe = Join-Path $PublishDir 'ShaPrint.UI.exe'
if (-not (Test-Path $Exe)) { throw "Published exe not found: $Exe" }

$Zip = Join-Path $OutDir "shaprint-win-x64-v$Version.zip"
if (Test-Path $Zip) { Remove-Item $Zip }
Compress-Archive -Path $Exe -DestinationPath $Zip -CompressionLevel Optimal

Write-Host "==> Artifact ready: $Zip"
