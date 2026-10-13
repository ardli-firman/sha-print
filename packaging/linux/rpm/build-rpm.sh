#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "${SCRIPT_DIR}/../../.." && pwd)"
TAURI_DIR="${REPO_ROOT}/apps/desktop/src-tauri"
PACKAGING_DIR="${REPO_ROOT}/packaging/linux"

VERSION="$(sed -n 's/^version = "\(.*\)"/\1/p' "${TAURI_DIR}/Cargo.toml" | head -n 1)"
if [[ -z "${VERSION}" ]]; then
    echo "error: could not determine package version from ${TAURI_DIR}/Cargo.toml" >&2
    exit 1
fi

if [[ -z "${SHAPRINTD_BIN:-}" || -z "${SHAPRINTCTL_BIN:-}" ]]; then
    cargo build \
        --manifest-path "${TAURI_DIR}/Cargo.toml" \
        --release \
        --no-default-features \
        --bin shaprintd \
        --bin shaprintctl >&2
    SHAPRINTD_BIN="${TAURI_DIR}/target/release/shaprintd"
    SHAPRINTCTL_BIN="${TAURI_DIR}/target/release/shaprintctl"
fi

OUTPUT_DIR="${RPM_OUTPUT_DIR:-${TAURI_DIR}/target/rpm}"
RPMBUILD_TOPDIR="$(mktemp -d "${TMPDIR:-/tmp}/shaprint-rpmbuild.XXXXXX")"
trap 'rm -rf "${RPMBUILD_TOPDIR}"' EXIT

mkdir -p "${RPMBUILD_TOPDIR}"/{BUILD,RPMS,SOURCES,SPECS,SRPMS} "${OUTPUT_DIR}"

cp "${SHAPRINTD_BIN}" "${RPMBUILD_TOPDIR}/SOURCES/shaprintd"
cp "${SHAPRINTCTL_BIN}" "${RPMBUILD_TOPDIR}/SOURCES/shaprintctl"
cp "${PACKAGING_DIR}/config.json" "${RPMBUILD_TOPDIR}/SOURCES/config.json"
cp "${PACKAGING_DIR}/polkit/org.shaprint.control.policy" "${RPMBUILD_TOPDIR}/SOURCES/org.shaprint.control.policy"
cp "${PACKAGING_DIR}/systemd/shaprintd.service" "${RPMBUILD_TOPDIR}/SOURCES/shaprintd.service"
cp "${PACKAGING_DIR}/systemd/shaprint.sysusers" "${RPMBUILD_TOPDIR}/SOURCES/shaprint.sysusers"
cp "${PACKAGING_DIR}/systemd/shaprint.tmpfiles" "${RPMBUILD_TOPDIR}/SOURCES/shaprint.tmpfiles"
cp "${PACKAGING_DIR}/systemd/90-shaprint.preset" "${RPMBUILD_TOPDIR}/SOURCES/90-shaprint.preset"
cp "${SCRIPT_DIR}/shaprint.spec" "${RPMBUILD_TOPDIR}/SPECS/shaprint.spec"

rpmbuild -bb \
    --define "_topdir ${RPMBUILD_TOPDIR}" \
    --define "_shaprint_version ${VERSION}" \
    "${RPMBUILD_TOPDIR}/SPECS/shaprint.spec" >&2

BUILT_RPM="$(find "${RPMBUILD_TOPDIR}/RPMS" -type f -name "shaprint-${VERSION}-*.rpm" | head -n 1)"
if [[ -z "${BUILT_RPM}" ]]; then
    echo "error: rpmbuild did not produce an RPM artifact" >&2
    exit 1
fi

FINAL_RPM="${OUTPUT_DIR}/$(basename "${BUILT_RPM}")"
cp "${BUILT_RPM}" "${FINAL_RPM}"
echo "${FINAL_RPM}"
