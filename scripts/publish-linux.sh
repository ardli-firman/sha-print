#!/usr/bin/env bash
# Publish ShaPrint.UI (Avalonia) for linux-x64 as a self-contained single-file binary,
# packed into artifacts/<version>/shaprint-linux-x64-v<version>.tar.gz.
# Run ON a Linux host.
#
# Usage:
#   ./scripts/publish-linux.sh [version]
#
# version is optional. Default: `git describe --tags --always` (leading 'v' stripped,
# since NuGet/MSBuild reject 'v1.2.3' as a -p:Version), falling back to a DOTTED
# date+short-sha (NuGet requires Major.Minor...). The value is sanitized to
# [A-Za-z0-9._-] and validated as X.Y[.Z[.R]][-suffix]. No secrets, no hardcoded
# paths, no admin rights required.

set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"

VERSION="${1:-}"
if [ -z "$VERSION" ]; then
  VERSION="$(git describe --tags --always 2>/dev/null | sed 's/^v//' || true)"
  if [ -z "$VERSION" ]; then
    VERSION="$(date +'%Y.%m.%d')-$(git rev-parse --short HEAD 2>/dev/null || echo unknown)"
  fi
fi
# Sanitize: keep only letters/digits/'.'/'_'/'-' (defensive even for explicit input).
VERSION="$(printf '%s' "$VERSION" | tr -c 'A-Za-z0-9._-' '-')"
if ! printf '%s' "$VERSION" | grep -Eq '^[0-9]+(\.[0-9]+){1,3}(-[A-Za-z0-9][A-Za-z0-9._-]*)?$'; then
  echo "ERROR: version '$VERSION' is not valid for -p:Version (expected X.Y[.Z[.R]][-suffix], e.g. 1.0.0-test)." >&2
  exit 1
fi

OUT_DIR="$ROOT/artifacts/$VERSION"
mkdir -p "$OUT_DIR"

PROJ="$ROOT/ShaPrint.UI/ShaPrint.UI.csproj"
# Locate a dotnet host: PATH first, then default install locations (install-script
# layout ~/.dotnet, distro package layout /usr/local/share/dotnet).
if ! command -v dotnet >/dev/null 2>&1; then
  for c in /usr/local/share/dotnet/dotnet "$HOME/.dotnet/dotnet"; do
    if [ -x "$c" ]; then export PATH="$(dirname "$c"):$PATH"; break; fi
  done
fi
if ! command -v dotnet >/dev/null 2>&1; then
  echo "ERROR: dotnet not found on PATH. Install the .NET 8 SDK (see docs/manual-artifacts-build.md)." >&2
  exit 1
fi

echo "==> Publishing ShaPrint.UI for linux-x64 — version: $VERSION"
dotnet publish "$PROJ" -f net8.0 -c Release -r linux-x64 --self-contained true \
  -p:PublishSingleFile=true -p:IncludeNativeLibrariesForSelfExtract=true -p:Version="$VERSION"

BINARY="$ROOT/ShaPrint.UI/bin/Release/net8.0/linux-x64/publish/ShaPrint.UI"
if [ ! -f "$BINARY" ]; then
  echo "ERROR: published binary not found: $BINARY" >&2
  exit 1
fi

TARBALL="$OUT_DIR/shaprint-linux-x64-v$VERSION.tar.gz"
(cd "$(dirname "$BINARY")" && tar czf "$TARBALL" "$(basename "$BINARY")")
echo "==> Artifact ready: $TARBALL"
