#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "${SCRIPT_DIR}/../.." && pwd)"
KEYS_DIR="${REPO_ROOT}/packaging/linux/keys"

CHANNEL="stable"
RPM_DIR=""
DEB_DIR=""
OUTPUT_DIR=""
KEY_FILE="${KEYS_DIR}/shaprint-repo.sec.key"
NOTICE_DISTRO=""
BASE_URL=""

while [[ $# -gt 0 ]]; do
    case "$1" in
        --channel)
            CHANNEL="$2"
            shift 2
            ;;
        --rpm-dir)
            RPM_DIR="$2"
            shift 2
            ;;
        --deb-dir)
            DEB_DIR="$2"
            shift 2
            ;;
        --output-dir)
            OUTPUT_DIR="$2"
            shift 2
            ;;
        --key-file)
            KEY_FILE="$2"
            shift 2
            ;;
        --notice-distro)
            NOTICE_DISTRO="$2"
            shift 2
            ;;
        --base-url)
            BASE_URL="$2"
            shift 2
            ;;
        *)
            echo "error: unrecognized argument $1" >&2
            exit 1
            ;;
    esac
done

if [[ -z "${BASE_URL}" ]]; then
    BASE_URL="https://github.com/ardli-firman/sha-print/releases/download/${CHANNEL}"
fi

if [[ -z "${OUTPUT_DIR}" ]]; then
    OUTPUT_DIR="${REPO_ROOT}/target/repositories/${CHANNEL}"
fi

mkdir -p "${OUTPUT_DIR}"

GNUPGHOME="$(mktemp -d "${TMPDIR:-/tmp}/shaprint-repo-gpg.XXXXXX")"
export GNUPGHOME
TEMP_STAGE="$(mktemp -d "${TMPDIR:-/tmp}/shaprint-stage.XXXXXX")"
trap 'rm -rf "${GNUPGHOME}" "${TEMP_STAGE}"' EXIT

if [[ -f "${KEY_FILE}" ]]; then
    gpg --batch --import "${KEY_FILE}" >&2
    KEY_ID="$(gpg --list-secret-keys --with-colons | grep '^sec:' | cut -d: -f5 | head -n 1)"
else
    echo "GPG secret key not found at ${KEY_FILE}, generating ephemeral test key pair in temp keyring..." >&2
    gpg --batch --passphrase '' --quick-generate-key "ShaPrint Repository Signing Key <maintainers@shaprint.local>" default default 0 >&2
    KEY_ID="$(gpg --list-secret-keys --with-colons | grep '^sec:' | cut -d: -f5 | head -n 1)"
fi

if [[ -z "${KEY_ID}" ]]; then
    echo "error: could not determine GPG key ID" >&2
    exit 1
fi

# === 1. BUILD RPM REPOSITORY ===
RPM_STAGE="${TEMP_STAGE}/rpm"
mkdir -p "${RPM_STAGE}"

if [[ -n "${RPM_DIR}" && -d "${RPM_DIR}" ]]; then
    find "${RPM_DIR}" -type f -name "*.rpm" -exec cp {} "${RPM_STAGE}/" \;
fi

# Resolve container CLI once
resolve_container_cli() {
    if command -v podman >/dev/null 2>&1; then
        echo "podman"
    elif command -v docker >/dev/null 2>&1; then
        echo "docker"
    fi
}
CONTAINER_CLI="$(resolve_container_cli)"

# Cryptographically sign individual RPM packages with GPG key
if ls "${RPM_STAGE}"/*.rpm >/dev/null 2>&1; then
    if command -v rpmsign >/dev/null 2>&1; then
        rpmsign --define "_gpg_name ${KEY_ID}" --define "_gpg_digest_algo sha256" --addsign "${RPM_STAGE}"/*.rpm >&2
    elif [[ -n "${CONTAINER_CLI}" ]]; then
        ${CONTAINER_CLI} run --rm \
            -v "${RPM_STAGE}:/rpm:rw,z" \
            -v "${GNUPGHOME}:/root/.gnupg:rw,z" \
            docker.io/library/almalinux:10 bash -euo pipefail -c "
                dnf install -y rpm-sign gnupg2 >/dev/null
                rpmsign --define '_gpg_name ${KEY_ID}' --define '_gpg_digest_algo sha256' --addsign /rpm/*.rpm
            " >&2
    else
        echo "error: neither rpmsign nor podman/docker found for RPM signing" >&2
        exit 1
    fi
fi

# Run createrepo_c
if command -v createrepo_c >/dev/null 2>&1; then
    createrepo_c "${RPM_STAGE}" >&2
else
    if [[ -n "${CONTAINER_CLI}" ]]; then
        ${CONTAINER_CLI} run --rm \
            -v "${RPM_STAGE}:/rpm:z" \
            docker.io/library/almalinux:10 bash -c "
                dnf install -y createrepo_c >/dev/null 2>&1
                createrepo_c /rpm
            " >&2
    else
        echo "error: neither createrepo_c nor podman/docker found" >&2
        exit 1
    fi
fi

# Sign repomd.xml
REPOMD="${RPM_STAGE}/repodata/repomd.xml"
if [[ -f "${REPOMD}" ]]; then
    gpg --batch --yes --pinentry-mode loopback --passphrase '' \
        --detach-sign --armor --local-user "${KEY_ID}" \
        --output "${REPOMD}.asc" "${REPOMD}" >&2
fi

# Generate .repo file
REPO_NOTICE_COMMENT=""
if [[ -n "${NOTICE_DISTRO}" ]]; then
    REPO_NOTICE_COMMENT="# NOTICE: ${NOTICE_DISTRO} is scheduled to leave the support matrix in the next stable release."
fi

cat <<EOF > "${RPM_STAGE}/shaprint-${CHANNEL}.repo"
[shaprint-${CHANNEL}]
name=ShaPrint ${CHANNEL} Repository
baseurl=${BASE_URL}/rpm
enabled=1
gpgcheck=1
repo_gpgcheck=1
gpgkey=https://raw.githubusercontent.com/ardli-firman/sha-print/main/packaging/linux/keys/shaprint-repo.gpg.key
${REPO_NOTICE_COMMENT}
EOF

# === 2. BUILD DEB REPOSITORY ===
DEB_STAGE="${TEMP_STAGE}/deb"
mkdir -p "${DEB_STAGE}"

if [[ -n "${DEB_DIR}" && -d "${DEB_DIR}" ]]; then
    find "${DEB_DIR}" -type f -name "*.deb" -exec cp {} "${DEB_STAGE}/" \;
fi

if command -v apt-ftparchive >/dev/null 2>&1; then
    (
        cd "${DEB_STAGE}"
        apt-ftparchive packages . > Packages
        gzip -9c Packages > Packages.gz
        apt-ftparchive release -o APT::FTPArchive::Release::Origin="ShaPrint" \
            -o APT::FTPArchive::Release::Label="ShaPrint" \
            -o APT::FTPArchive::Release::Suite="${CHANNEL}" \
            -o APT::FTPArchive::Release::Codename="${CHANNEL}" \
            -o APT::FTPArchive::Release::Architectures="amd64" \
            -o APT::FTPArchive::Release::Components="main" \
            -o APT::FTPArchive::Release::Description="ShaPrint ${CHANNEL} package repository" \
            . > Release
    )
else
    CONTAINER_CLI=""
    if command -v podman >/dev/null 2>&1; then
        CONTAINER_CLI="podman"
    elif command -v docker >/dev/null 2>&1; then
        CONTAINER_CLI="docker"
    fi

    if [[ -n "${CONTAINER_CLI}" ]]; then
        ${CONTAINER_CLI} run --rm \
            -v "${DEB_STAGE}:/deb:rw,z" \
            docker.io/library/debian:12 bash -c "
                cd /deb
                apt-get update >/dev/null 2>&1
                apt-get install -y --no-install-recommends apt-utils >/dev/null 2>&1
                apt-ftparchive packages . > Packages
                gzip -9c Packages > Packages.gz
                apt-ftparchive release -o APT::FTPArchive::Release::Origin='ShaPrint' \
                    -o APT::FTPArchive::Release::Label='ShaPrint' \
                    -o APT::FTPArchive::Release::Suite='${CHANNEL}' \
                    -o APT::FTPArchive::Release::Codename='${CHANNEL}' \
                    -o APT::FTPArchive::Release::Architectures='amd64' \
                    -o APT::FTPArchive::Release::Components='main' \
                    -o APT::FTPArchive::Release::Description='ShaPrint ${CHANNEL} package repository' \
                    . > Release
            " >&2
    else
        echo "error: neither apt-ftparchive nor podman/docker found" >&2
        exit 1
    fi
fi

RELEASE_FILE="${DEB_STAGE}/Release"
if [[ -n "${NOTICE_DISTRO}" ]]; then
    echo "Notice: ${NOTICE_DISTRO} is scheduled to leave the support matrix in the next stable release." >> "${RELEASE_FILE}"
fi

# Sign Release -> InRelease and Release.gpg
gpg --batch --yes --pinentry-mode loopback --passphrase '' \
    --clearsign --local-user "${KEY_ID}" \
    --output "${DEB_STAGE}/InRelease" "${RELEASE_FILE}" >&2

gpg --batch --yes --pinentry-mode loopback --passphrase '' \
    --detach-sign --armor --local-user "${KEY_ID}" \
    --output "${DEB_STAGE}/Release.gpg" "${RELEASE_FILE}" >&2

# Generate .sources file
cat <<EOF > "${DEB_STAGE}/shaprint-${CHANNEL}.sources"
Types: deb
URIs: ${BASE_URL}/deb
Suites: ./
Signed-By: /etc/apt/keyrings/shaprint-archive-keyring.gpg
${REPO_NOTICE_COMMENT}
EOF

# === 3. ATOMIC VERIFICATION BEFORE PUBLISHING ===
# Verify RPM metadata signature
if [[ -f "${REPOMD}.asc" ]]; then
    gpg --batch --verify "${REPOMD}.asc" "${REPOMD}" >&2 || {
        echo "error: failed to verify repomd.xml.asc signature" >&2
        exit 1
    }
fi

# Verify DEB metadata signatures
gpg --batch --verify "${DEB_STAGE}/InRelease" >&2 || {
    echo "error: failed to verify InRelease signature" >&2
    exit 1
}

gpg --batch --verify "${DEB_STAGE}/Release.gpg" "${RELEASE_FILE}" >&2 || {
    echo "error: failed to verify Release.gpg signature" >&2
    exit 1
}

# Atomically replace OUTPUT_DIR with verified staged repository
OUTPUT_PARENT="$(dirname "${OUTPUT_DIR}")"
mkdir -p "${OUTPUT_PARENT}"
PUBLISH_TMP="$(mktemp -d "${OUTPUT_PARENT}/.pub-tmp.XXXXXX")"
mkdir -p "${PUBLISH_TMP}/rpm" "${PUBLISH_TMP}/deb"
cp -r "${RPM_STAGE}"/* "${PUBLISH_TMP}/rpm/"
cp -r "${DEB_STAGE}"/* "${PUBLISH_TMP}/deb/"

rm -rf "${OUTPUT_DIR}"
mv "${PUBLISH_TMP}" "${OUTPUT_DIR}"

echo "Successfully generated and signed ${CHANNEL} repository at ${OUTPUT_DIR}"
