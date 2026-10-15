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

ALMALINUX_MAJOR="${ALMALINUX_MAJOR:-}"
RPM_DIST="${RPM_DIST:-}"
if [[ -n "${ALMALINUX_MAJOR}" && -z "${RPM_DIST}" ]]; then
    RPM_DIST="el${ALMALINUX_MAJOR}"
fi

if [[ -z "${SHAPRINTD_BIN:-}" || -z "${SHAPRINTCTL_BIN:-}" ]]; then
    # When targeting the previous AlmaLinux major (9) on a host with a newer glibc,
    # use containerized cargo build if container runtime is available to avoid raising
    # the runtime baseline beyond glibc 2.34.
    HOST_GLIBC_VERSION="$(ldd --version 2>/dev/null | head -n 1 | grep -oE '[0-9]+\.[0-9]+' | head -n 1 || true)"
    CONTAINER_CLI=""
    if command -v podman >/dev/null 2>&1; then
        CONTAINER_CLI="podman"
    elif command -v docker >/dev/null 2>&1; then
        CONTAINER_CLI="docker"
    fi

    if [[ ("${ALMALINUX_MAJOR}" == "9" || "${RPM_DIST}" == "el9") && -n "${CONTAINER_CLI}" && -n "${HOST_GLIBC_VERSION}" ]]; then
        MAJOR_PART="${HOST_GLIBC_VERSION%%.*}"
        MINOR_PART="${HOST_GLIBC_VERSION#*.}"
        if [[ "${MAJOR_PART}" -gt 2 || ("${MAJOR_PART}" -eq 2 && "${MINOR_PART}" -gt 34) ]]; then
            EL9_TARGET_DIR="${TAURI_DIR}/target/el9"
            mkdir -p "${EL9_TARGET_DIR}"
            ${CONTAINER_CLI} run --rm \
                -v "${REPO_ROOT}:/src:ro,z" \
                -v "${EL9_TARGET_DIR}:/target:z" \
                docker.io/library/almalinux:9 bash -c "
                    dnf install -y cargo rust gcc >/dev/null 2>&1
                    cargo build \
                        --manifest-path /src/apps/desktop/src-tauri/Cargo.toml \
                        --target-dir /target \
                        --release \
                        --no-default-features \
                        --bin shaprintd \
                        --bin shaprintctl
                " >&2
            SHAPRINTD_BIN="${EL9_TARGET_DIR}/release/shaprintd"
            SHAPRINTCTL_BIN="${EL9_TARGET_DIR}/release/shaprintctl"
        fi
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
fi

OUTPUT_DIR="${RPM_OUTPUT_DIR:-${TAURI_DIR}/target/rpm}"
RPMBUILD_TOPDIR="$(mktemp -d "${TMPDIR:-/tmp}/shaprint-rpmbuild.XXXXXX")"
trap 'rm -rf "${RPMBUILD_TOPDIR}"' EXIT

RELEASE="${RPM_RELEASE:-1}"

mkdir -p "${RPMBUILD_TOPDIR}"/{BUILD,RPMS,SOURCES,SPECS,SRPMS} "${OUTPUT_DIR}"

cp "${SHAPRINTD_BIN}" "${RPMBUILD_TOPDIR}/SOURCES/shaprintd"
cp "${SHAPRINTCTL_BIN}" "${RPMBUILD_TOPDIR}/SOURCES/shaprintctl"

if [[ -z "${SHAPRINT_DESKTOP_BIN:-}" ]]; then
    if [[ -f "${TAURI_DIR}/target/release/shaprint-desktop" ]]; then
        SHAPRINT_DESKTOP_BIN="${TAURI_DIR}/target/release/shaprint-desktop"
    fi
fi
if [[ -n "${SHAPRINT_DESKTOP_BIN:-}" && -f "${SHAPRINT_DESKTOP_BIN}" ]]; then
    cp "${SHAPRINT_DESKTOP_BIN}" "${RPMBUILD_TOPDIR}/SOURCES/shaprint-desktop"
fi

if [[ -f "${PACKAGING_DIR}/desktop/shaprint.desktop" ]]; then
    cp "${PACKAGING_DIR}/desktop/shaprint.desktop" "${RPMBUILD_TOPDIR}/SOURCES/shaprint.desktop"
fi

ICONS_DIR="${TAURI_DIR}/icons"
if [[ -d "${ICONS_DIR}" ]]; then
    cp "${ICONS_DIR}/32x32.png" "${RPMBUILD_TOPDIR}/SOURCES/icon-32x32.png" 2>/dev/null || :
    cp "${ICONS_DIR}/128x128.png" "${RPMBUILD_TOPDIR}/SOURCES/icon-128x128.png" 2>/dev/null || :
    cp "${ICONS_DIR}/128x128@2x.png" "${RPMBUILD_TOPDIR}/SOURCES/icon-256x256.png" 2>/dev/null || :
    cp "${ICONS_DIR}/icon.png" "${RPMBUILD_TOPDIR}/SOURCES/icon-512x512.png" 2>/dev/null || :
fi

cp "${PACKAGING_DIR}/config.json" "${RPMBUILD_TOPDIR}/SOURCES/config.json"
cp "${PACKAGING_DIR}/polkit/org.shaprint.control.policy" "${RPMBUILD_TOPDIR}/SOURCES/org.shaprint.control.policy"
cp "${PACKAGING_DIR}/systemd/shaprintd.service" "${RPMBUILD_TOPDIR}/SOURCES/shaprintd.service"
cp "${PACKAGING_DIR}/systemd/shaprint.sysusers" "${RPMBUILD_TOPDIR}/SOURCES/shaprint.sysusers"
cp "${PACKAGING_DIR}/systemd/shaprint.tmpfiles" "${RPMBUILD_TOPDIR}/SOURCES/shaprint.tmpfiles"
cp "${PACKAGING_DIR}/systemd/90-shaprint.preset" "${RPMBUILD_TOPDIR}/SOURCES/90-shaprint.preset"
cp "${SCRIPT_DIR}/shaprint.spec" "${RPMBUILD_TOPDIR}/SPECS/shaprint.spec"

RPMBUILD_ARGS=(
    -bb
    --define "_topdir ${RPMBUILD_TOPDIR}"
    --define "_shaprint_version ${VERSION}"
    --define "_shaprint_release ${RELEASE}"
)
if [[ -n "${RPM_DIST}" ]]; then
    RPMBUILD_ARGS+=(-D "dist .${RPM_DIST#.}")
fi

rpmbuild "${RPMBUILD_ARGS[@]}" \
    "${RPMBUILD_TOPDIR}/SPECS/shaprint.spec" >&2

BUILT_RPM="$(find "${RPMBUILD_TOPDIR}/RPMS" -type f -name "shaprint-${VERSION}-*.rpm" | head -n 1)"
if [[ -z "${BUILT_RPM}" ]]; then
    echo "error: rpmbuild did not produce an RPM artifact" >&2
    exit 1
fi

FINAL_RPM="${OUTPUT_DIR}/$(basename "${BUILT_RPM}")"
cp "${BUILT_RPM}" "${FINAL_RPM}"
echo "${FINAL_RPM}"
