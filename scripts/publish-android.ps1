#!/usr/bin/env pwsh
# Build the ShaPrint.Android Release APK and copy it into
# artifacts/<version>/shaprint-android-v<version>.apk.
# Run on a Windows host (or any host with the android workload + SDK + JDK).
#
# Usage:
#   ./scripts/publish-android.ps1 [-Version <version>]
#
# -Version is optional. Default: `git describe --tags --always` (leading 'v' stripped,
# since NuGet/MSBuild reject 'v1.2.3' as a -p:Version), falling back to a DOTTED
# date+short-sha (NuGet requires Major.Minor...). The value is sanitized to
# [A-Za-z0-9._-] and validated as X.Y[.Z[.R]][-suffix]. The APK keeps the app
# version baked into the csproj (<Version>2.0.0</Version>); the artifact file name /
# folder use the artifact version.
#
# Honors $env:ANDROID_HOME / $env:JAVA_HOME when set (otherwise .NET Android uses its
# default SDK location). The APK is signed with the debug keystore unless real signing
# properties are configured — see docs/manual-artifacts-build.md (Android signing note).

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

$Proj = Join-Path $RepoRoot 'ShaPrint.Android\ShaPrint.Android.csproj'
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

# Honor SDK/JDK env vars (the build reads them itself; shown here for visibility).
if ($env:ANDROID_HOME) { Write-Host "==> ANDROID_HOME: $env:ANDROID_HOME" }
if ($env:JAVA_HOME)    { Write-Host "==> JAVA_HOME:   $env:JAVA_HOME" }

# 1. Install any missing Android SDK components (API 34 platform, build-tools, ...)
#    and accept the licenses non-interactively.
Write-Host "==> Installing missing Android SDK dependencies (licenses accepted)"
& $DotNet build $Proj -f net8.0-android -c Release -t:InstallAndroidDependencies -p:AcceptAndroidSdkLicenses=true
if ($LASTEXITCODE -ne 0) { throw "InstallAndroidDependencies failed with exit code $LASTEXITCODE" }

# 2. Normal Release build (trimming + profiled AOT; produces the Signed APK).
Write-Host "==> Building ShaPrint.Android Release APK (artifact version: $Version)"
& $DotNet build $Proj -f net8.0-android -c Release
if ($LASTEXITCODE -ne 0) { throw "Release build failed with exit code $LASTEXITCODE" }

# 3. Copy the signed APK into artifacts/<version>/.
$Apk = Join-Path $RepoRoot 'ShaPrint.Android\bin\Release\net8.0-android\com.shaprint.android-Signed.apk'
if (-not (Test-Path $Apk)) {
    throw "APK not found after build: $Apk`nThe Release build did not produce com.shaprint.android-Signed.apk. " +
          "Check the build output above (e.g. debug-keystore signing failure) and that the android workload + SDK are installed."
}

$Dest = Join-Path $OutDir "shaprint-android-v$Version.apk"
Copy-Item -Path $Apk -Destination $Dest -Force

Write-Host "==> Artifact ready: $Dest"
