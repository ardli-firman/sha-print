#!/usr/bin/env bash
set -euo pipefail

: "${NIGHTLY_TAG:?NIGHTLY_TAG must be set}"
: "${TARGET_SHA:?TARGET_SHA must be set}"

git -c user.name="github-actions[bot]" \
  -c user.email="41898282+github-actions[bot]@users.noreply.github.com" \
  tag -a "$NIGHTLY_TAG" "$TARGET_SHA" -m "ShaPrint $NIGHTLY_TAG"
