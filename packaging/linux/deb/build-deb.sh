#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "${SCRIPT_DIR}/../../.." && pwd)"
TAURI_DIR="${REPO_ROOT}/apps/desktop/src-tauri"
PACKAGING_DIR="${REPO_ROOT}/packaging/linux"
DEB_DIR="${PACKAGING_DIR}/deb"

VERSION="$(sed -n 's/^version = "\(.*\)"/\1/p' "${TAURI_DIR}/Cargo.toml" | head -n 1)"
if [[ -z "${VERSION}" ]]; then
    echo "error: could not determine package version from ${TAURI_DIR}/Cargo.toml" >&2
    exit 1
fi

RELEASE="${DEB_RELEASE:-1}"
OUTPUT_DIR="${DEB_OUTPUT_DIR:-${TAURI_DIR}/target/deb}"
mkdir -p "${OUTPUT_DIR}"

if [[ -z "${SHAPRINTD_BIN:-}" || -z "${SHAPRINTCTL_BIN:-}" ]]; then
    # To support both the primary (24.04) and previous (22.04) Ubuntu LTS releases,
    # the runtime baseline must not exceed glibc 2.35.
    HOST_GLIBC_VERSION="$(ldd --version 2>/dev/null | head -n 1 | grep -oE '[0-9]+\.[0-9]+' | head -n 1 || true)"
    CONTAINER_CLI=""
    if command -v podman >/dev/null 2>&1; then
        CONTAINER_CLI="podman"
    elif command -v docker >/dev/null 2>&1; then
        CONTAINER_CLI="docker"
    fi

    # Check if pre-compiled baseline binary with glibc <= 2.35 is already available
    if [[ -f "${TAURI_DIR}/target/deb-baseline/release/shaprintd" && -f "${TAURI_DIR}/target/deb-baseline/release/shaprintctl" ]]; then
        SHAPRINTD_BIN="${TAURI_DIR}/target/deb-baseline/release/shaprintd"
        SHAPRINTCTL_BIN="${TAURI_DIR}/target/deb-baseline/release/shaprintctl"
    elif [[ -f "${TAURI_DIR}/target/el9/release/shaprintd" && -f "${TAURI_DIR}/target/el9/release/shaprintctl" ]]; then
        SHAPRINTD_BIN="${TAURI_DIR}/target/el9/release/shaprintd"
        SHAPRINTCTL_BIN="${TAURI_DIR}/target/el9/release/shaprintctl"
    elif [[ -n "${CONTAINER_CLI}" && -n "${HOST_GLIBC_VERSION}" ]]; then
        MAJOR_PART="${HOST_GLIBC_VERSION%%.*}"
        MINOR_PART="${HOST_GLIBC_VERSION#*.}"
        if [[ "${MAJOR_PART}" -gt 2 || ("${MAJOR_PART}" -eq 2 && "${MINOR_PART}" -gt 35) ]]; then
            DEB_BUILD_TARGET_DIR="${TAURI_DIR}/target/deb-baseline"
            mkdir -p "${DEB_BUILD_TARGET_DIR}"
            ${CONTAINER_CLI} run --rm \
                -v "${REPO_ROOT}:/src:ro,z" \
                -v "${DEB_BUILD_TARGET_DIR}:/target:z" \
                docker.io/library/ubuntu:22.04 bash -c "
                    apt-get update >/dev/null 2>&1 && \
                    apt-get install -y --no-install-recommends cargo rustc gcc libc6-dev ca-certificates >/dev/null 2>&1
                    cargo build \
                        --manifest-path /src/apps/desktop/src-tauri/Cargo.toml \
                        --target-dir /target \
                        --release \
                        --no-default-features \
                        --bin shaprintd \
                        --bin shaprintctl
                " >&2
            if [[ -f "${DEB_BUILD_TARGET_DIR}/release/shaprintd" && -f "${DEB_BUILD_TARGET_DIR}/release/shaprintctl" ]]; then
                SHAPRINTD_BIN="${DEB_BUILD_TARGET_DIR}/release/shaprintd"
                SHAPRINTCTL_BIN="${DEB_BUILD_TARGET_DIR}/release/shaprintctl"
            fi
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

STAGING_DIR="$(mktemp -d "${TMPDIR:-/tmp}/shaprint-deb-build.XXXXXX")"
trap 'rm -rf "${STAGING_DIR}"' EXIT

mkdir -p \
    "${STAGING_DIR}/DEBIAN" \
    "${STAGING_DIR}/usr/bin" \
    "${STAGING_DIR}/usr/lib/systemd/system" \
    "${STAGING_DIR}/usr/lib/sysusers.d" \
    "${STAGING_DIR}/usr/lib/tmpfiles.d" \
    "${STAGING_DIR}/usr/lib/systemd/system-preset" \
    "${STAGING_DIR}/etc/shaprint" \
    "${STAGING_DIR}/usr/share/polkit-1/actions"

# Control and maintainer scripts
sed \
    -e "s/@VERSION@/${VERSION}/g" \
    -e "s/@RELEASE@/${RELEASE}/g" \
    "${DEB_DIR}/control" > "${STAGING_DIR}/DEBIAN/control"
chmod 0644 "${STAGING_DIR}/DEBIAN/control"

cp "${DEB_DIR}/preinst" "${STAGING_DIR}/DEBIAN/preinst"
cp "${DEB_DIR}/postinst" "${STAGING_DIR}/DEBIAN/postinst"
cp "${DEB_DIR}/prerm" "${STAGING_DIR}/DEBIAN/prerm"
cp "${DEB_DIR}/postrm" "${STAGING_DIR}/DEBIAN/postrm"
chmod 0755 "${STAGING_DIR}/DEBIAN"/preinst "${STAGING_DIR}/DEBIAN"/postinst "${STAGING_DIR}/DEBIAN"/prerm "${STAGING_DIR}/DEBIAN"/postrm

# Binaries and system integration
cp "${SHAPRINTD_BIN}" "${STAGING_DIR}/usr/bin/shaprintd"
cp "${SHAPRINTCTL_BIN}" "${STAGING_DIR}/usr/bin/shaprintctl"
chmod 0755 "${STAGING_DIR}/usr/bin"/shaprintd "${STAGING_DIR}/usr/bin"/shaprintctl

cp "${PACKAGING_DIR}/systemd/shaprintd.service" "${STAGING_DIR}/usr/lib/systemd/system/shaprintd.service"
chmod 0644 "${STAGING_DIR}/usr/lib/systemd/system/shaprintd.service"

cp "${PACKAGING_DIR}/systemd/shaprint.sysusers" "${STAGING_DIR}/usr/lib/sysusers.d/shaprint.conf"
chmod 0644 "${STAGING_DIR}/usr/lib/sysusers.d/shaprint.conf"

cp "${PACKAGING_DIR}/systemd/shaprint.tmpfiles" "${STAGING_DIR}/usr/lib/tmpfiles.d/shaprint.conf"
chmod 0644 "${STAGING_DIR}/usr/lib/tmpfiles.d/shaprint.conf"

cp "${PACKAGING_DIR}/systemd/90-shaprint.preset" "${STAGING_DIR}/usr/lib/systemd/system-preset/90-shaprint.preset"
chmod 0644 "${STAGING_DIR}/usr/lib/systemd/system-preset/90-shaprint.preset"

cp "${PACKAGING_DIR}/config.json" "${STAGING_DIR}/etc/shaprint/config.json"
chmod 0640 "${STAGING_DIR}/etc/shaprint/config.json"

cp "${PACKAGING_DIR}/polkit/org.shaprint.control.policy" "${STAGING_DIR}/usr/share/polkit-1/actions/org.shaprint.control.policy"
chmod 0644 "${STAGING_DIR}/usr/share/polkit-1/actions/org.shaprint.control.policy"

DEB_NAME="shaprint_${VERSION}-${RELEASE}_amd64.deb"
FINAL_DEB="${OUTPUT_DIR}/${DEB_NAME}"

if command -v dpkg-deb >/dev/null 2>&1; then
    dpkg-deb --build --root-owner-group "${STAGING_DIR}" "${FINAL_DEB}" >&2
else
    # Fallback to Ubuntu container if host lacks dpkg-deb (e.g. RPM-based host)
    CONTAINER_CLI=""
    if command -v podman >/dev/null 2>&1; then
        CONTAINER_CLI="podman"
    elif command -v docker >/dev/null 2>&1; then
        CONTAINER_CLI="docker"
    fi

    if [[ -n "${CONTAINER_CLI}" ]]; then
        ${CONTAINER_CLI} run --rm \
            -v "${STAGING_DIR}:/staging:z" \
            -v "${OUTPUT_DIR}:/out:z" \
            docker.io/library/ubuntu:24.04 \
            dpkg-deb --build --root-owner-group /staging "/out/${DEB_NAME}" >&2
    else
        echo "error: neither dpkg-deb nor podman/docker found to build .deb package" >&2
        exit 1
    fi
fi

echo "${FINAL_DEB}"
