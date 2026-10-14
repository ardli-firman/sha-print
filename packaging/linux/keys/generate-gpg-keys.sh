#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
KEYS_DIR="${1:-${SCRIPT_DIR}}"
mkdir -p "${KEYS_DIR}"

GNUPGHOME="$(mktemp -d "${TMPDIR:-/tmp}/shaprint-gnupg.XXXXXX")"
export GNUPGHOME
trap 'rm -rf "${GNUPGHOME}"' EXIT

gpg --batch --passphrase '' --quick-generate-key "ShaPrint Repository Signing Key <maintainers@shaprint.local>" default default 0
gpg --armor --export "maintainers@shaprint.local" > "${KEYS_DIR}/shaprint-repo.gpg.key"
gpg --export "maintainers@shaprint.local" > "${KEYS_DIR}/shaprint-archive-keyring.gpg"
gpg --armor --export-secret-keys --batch --passphrase '' "maintainers@shaprint.local" > "${KEYS_DIR}/shaprint-repo.sec.key"

echo "Generated GPG repository signing keys in ${KEYS_DIR}"
