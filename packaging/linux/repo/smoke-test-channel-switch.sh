#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "${SCRIPT_DIR}/../../.." && pwd)"
KEYS_DIR="${REPO_ROOT}/packaging/linux/keys"
SCRIPTS_DIR="${REPO_ROOT}/scripts/linux-repo"

RPM_DIR="${REPO_ROOT}/apps/desktop/src-tauri/target/rpm"
DEB_DIR="${REPO_ROOT}/apps/desktop/src-tauri/target/deb"
REPO_BASE_DIR="$(mktemp -d "${TMPDIR:-/tmp}/shaprint-repos.XXXXXX")"
ALMA_CONTAINER=""
U24_CONTAINER=""

cleanup() {
    rm -rf "${REPO_BASE_DIR:-}"
    podman rm -f "${ALMA_CONTAINER:-}" "${U24_CONTAINER:-}" >/dev/null 2>&1 || true
}
trap cleanup EXIT

# Generate signed Stable and Nightly repositories
echo "=== 1. Generating signed Stable and Nightly repositories ==="
mkdir -p "${REPO_BASE_DIR}/stable-artifacts/rpm" "${REPO_BASE_DIR}/stable-artifacts/deb"
RPM_STABLE_PATH="$(RPM_RELEASE=1 "${REPO_ROOT}/packaging/linux/rpm/build-rpm.sh")"
cp "${RPM_STABLE_PATH}" "${REPO_BASE_DIR}/stable-artifacts/rpm/"

DEB_STABLE_PATH="$(DEB_RELEASE=1 "${REPO_ROOT}/packaging/linux/deb/build-deb.sh")"
cp "${DEB_STABLE_PATH}" "${REPO_BASE_DIR}/stable-artifacts/deb/"

bash "${SCRIPTS_DIR}/publish-repo.sh" \
    --channel stable \
    --rpm-dir "${REPO_BASE_DIR}/stable-artifacts/rpm" \
    --deb-dir "${REPO_BASE_DIR}/stable-artifacts/deb" \
    --output-dir "${REPO_BASE_DIR}/stable"

# Create a modified nightly RPM artifact to simulate channel switch
mkdir -p "${REPO_BASE_DIR}/nightly-artifacts/rpm" "${REPO_BASE_DIR}/nightly-artifacts/deb"
RPM_NIGHTLY_PATH="$(RPM_RELEASE=2 "${REPO_ROOT}/packaging/linux/rpm/build-rpm.sh")"
cp "${RPM_NIGHTLY_PATH}" "${REPO_BASE_DIR}/nightly-artifacts/rpm/"

DEB_NIGHTLY_PATH="$(DEB_RELEASE=2 "${REPO_ROOT}/packaging/linux/deb/build-deb.sh")"
cp "${DEB_NIGHTLY_PATH}" "${REPO_BASE_DIR}/nightly-artifacts/deb/"

bash "${SCRIPTS_DIR}/publish-repo.sh" \
    --channel nightly \
    --rpm-dir "${REPO_BASE_DIR}/nightly-artifacts/rpm" \
    --deb-dir "${REPO_BASE_DIR}/nightly-artifacts/deb" \
    --output-dir "${REPO_BASE_DIR}/nightly"

# Ensure repository and keys directories are readable by non-root container users (e.g. _apt)
chmod -R a+rX "${REPO_BASE_DIR}" "${KEYS_DIR}"

# === 2. DNF / RPM SMOKE TEST (AlmaLinux 10 Container) ===
echo "=== 2. Testing DNF Channel Switch & GPG Verification (AlmaLinux 10) ==="
ALMA_CONTAINER="shaprint-repo-alma10-$$"

podman run -d --name "${ALMA_CONTAINER}" \
    -v "${REPO_BASE_DIR}:/repos:ro,z" \
    -v "${KEYS_DIR}:/keys:ro,z" \
    docker.io/almalinux/10-init >/dev/null

wait_for_systemd() {
    local name="$1"
    for ((i = 1; i <= 40; i++)); do
        local state
        state="$(podman exec "${name}" systemctl is-system-running 2>/dev/null || true)"
        if [[ "${state}" == "running" || "${state}" == "degraded" ]]; then
            return 0
        fi
        sleep 0.25
    done
    echo "error: systemd inside ${name} did not reach running state" >&2
    return 1
}

wait_for_systemd "${ALMA_CONTAINER}"

podman exec "${ALMA_CONTAINER}" bash -euo pipefail -c "
    wait_for_shaprintd() {
        for _ in {1..40}; do
            if systemctl is-active --quiet shaprintd.service && [[ -S /run/shaprint/shaprintd.sock ]]; then
                if shaprintctl status 2>/dev/null | grep -q '^client-proxy: running$'; then
                    return 0
                fi
            fi
            sleep 0.25
        done
        echo 'error: shaprintd.service did not become active' >&2
        return 1
    }

    # 1. Configure Stable RPM repository with GPG verification enabled
    cat <<EOF > /etc/yum.repos.d/shaprint-stable.repo
[shaprint-stable]
name=ShaPrint Stable Repository
baseurl=file:///repos/stable/rpm
enabled=1
gpgcheck=1
repo_gpgcheck=1
gpgkey=file:///keys/shaprint-repo.gpg.key
EOF

    # Import GPG key
    rpm --import /keys/shaprint-repo.gpg.key

    # 2. Install cups and then install Stable package via DNF
    dnf install -y cups >/dev/null 2>&1 || true
    dnf install -y --enablerepo='shaprint-stable' --repo='shaprint-stable' --setopt=install_weak_deps=False shaprint || \
    dnf install -y --enablerepo='shaprint-stable' shaprint
    wait_for_shaprintd

    # 3. Configure state & credentials
    SECRET=\"stable-chan-\$\$-\$(date +%s)\"
    printf '%s\n' \"\${SECRET}\" | shaprintctl network-channel set --stdin
    CERT_SHA_STABLE=\$(sha256sum /var/lib/shaprint/server-identity.cert.der | awk '{print \$1}')

    # 4. Configure Nightly repository and upgrade (Channel Switch Stable -> Nightly)
    cat <<EOF > /etc/yum.repos.d/shaprint-nightly.repo
[shaprint-nightly]
name=ShaPrint Nightly Repository
baseurl=file:///repos/nightly/rpm
enabled=1
gpgcheck=1
repo_gpgcheck=1
gpgkey=file:///keys/shaprint-repo.gpg.key
EOF

    dnf --disablerepo='shaprint-stable' --enablerepo='shaprint-nightly' upgrade -y shaprint
    wait_for_shaprintd

    # Verify same package identity, preserved state, preserved credentials
    SHAPRINT_PKGS=\$(rpm -q shaprint | wc -l)
    if [[ \"\${SHAPRINT_PKGS}\" -ne 1 ]]; then
        echo \"error: expected exactly 1 installed shaprint package, got \${SHAPRINT_PKGS}\" >&2
        exit 1
    fi

    CERT_SHA_NIGHTLY=\$(sha256sum /var/lib/shaprint/server-identity.cert.der | awk '{print \$1}')
    if [[ \"\${CERT_SHA_STABLE}\" != \"\${CERT_SHA_NIGHTLY}\" ]]; then
        echo 'error: channel switch replaced server identity' >&2
        exit 1
    fi

    shaprintctl status | grep -q '^network-channel: configured$'

    # 5. Downgrade back from Nightly to Stable (Channel Switch Nightly -> Stable)
    dnf --disablerepo='shaprint-nightly' --enablerepo='shaprint-stable' downgrade -y shaprint
    wait_for_shaprintd

    CERT_SHA_DOWNGRADE=\$(sha256sum /var/lib/shaprint/server-identity.cert.der | awk '{print \$1}')
    if [[ \"\${CERT_SHA_STABLE}\" != \"\${CERT_SHA_DOWNGRADE}\" ]]; then
        echo 'error: downgrade replaced server identity' >&2
        exit 1
    fi
    shaprintctl status | grep -q '^network-channel: configured$'
    echo 'verified: DNF channel switch Stable <-> Nightly preserves single package identity and state'

    # 6. Verify security: corrupted/tampered metadata signature fails safely
    cat <<EOF > /etc/yum.repos.d/shaprint-tampered.repo
[shaprint-tampered]
name=ShaPrint Tampered Repository
baseurl=file:///repos/stable/rpm
enabled=1
gpgcheck=0
repo_gpgcheck=1
gpgkey=file:///dev/null
EOF
    if dnf --disablerepo='*' --enablerepo='shaprint-tampered' makecache 2>/dev/null; then
        echo 'error: dnf makecache with invalid gpg key must fail' >&2
        exit 1
    fi
    echo 'verified: DNF rejects invalid or untrusted repository signatures safely'
"

podman rm -f "${ALMA_CONTAINER}" >/dev/null

# === 3. APT / DEB SMOKE TEST (Ubuntu 24.04 Container) ===
echo "=== 3. Testing APT Channel Switch & GPG Verification (Ubuntu 24.04) ==="
U24_CONTAINER="shaprint-repo-u24-$$"

podman run -d --name "${U24_CONTAINER}" --cap-add=NET_ADMIN \
    -v "${REPO_BASE_DIR}:/repos:ro,z" \
    -v "${KEYS_DIR}:/keys:ro,z" \
    localhost/ubuntu:24.04-init >/dev/null

wait_for_systemd "${U24_CONTAINER}"

podman exec "${U24_CONTAINER}" bash -euo pipefail -c "
    wait_for_shaprintd() {
        for _ in {1..40}; do
            if systemctl is-active --quiet shaprintd.service && [[ -S /run/shaprint/shaprintd.sock ]]; then
                if shaprintctl status 2>/dev/null | grep -q '^client-proxy: running$'; then
                    return 0
                fi
            fi
            sleep 0.25
        done
        echo 'error: shaprintd.service did not become active' >&2
        return 1
    }

    # 1. Configure keyrings and sources
    mkdir -p /etc/apt/keyrings
    cp /keys/shaprint-archive-keyring.gpg /etc/apt/keyrings/shaprint-archive-keyring.gpg

    echo 'deb [signed-by=/etc/apt/keyrings/shaprint-archive-keyring.gpg] file:///repos/stable/deb ./' > /etc/apt/sources.list.d/shaprint.list

    # 2. apt-get update & install
    apt-get update
    apt-get install -y --allow-unauthenticated=false shaprint
    wait_for_shaprintd
    STABLE_VERSION=\$(dpkg-query -W -f='\${Version}' shaprint)

    # 3. Configure state
    SECRET=\"deb-chan-\$\$-\$(date +%s)\"
    printf '%s\n' \"\${SECRET}\" | shaprintctl network-channel set --stdin
    CERT_SHA_STABLE=\$(sha256sum /var/lib/shaprint/server-identity.cert.der | awk '{print \$1}')

    # 4. Switch repository to Nightly and upgrade
    echo 'deb [signed-by=/etc/apt/keyrings/shaprint-archive-keyring.gpg] file:///repos/nightly/deb ./' > /etc/apt/sources.list.d/shaprint.list
    apt-get update
    apt-get install -y --only-upgrade shaprint
    wait_for_shaprintd

    CERT_SHA_NIGHTLY=\$(sha256sum /var/lib/shaprint/server-identity.cert.der | awk '{print \$1}')
    if [[ \"\${CERT_SHA_STABLE}\" != \"\${CERT_SHA_NIGHTLY}\" ]]; then
        echo 'error: apt channel switch replaced server identity' >&2
        exit 1
    fi
    shaprintctl status | grep -q '^network-channel: configured$'

    # 5. Downgrade back to Stable
    echo 'deb [signed-by=/etc/apt/keyrings/shaprint-archive-keyring.gpg] file:///repos/stable/deb ./' > /etc/apt/sources.list.d/shaprint.list
    apt-get update
    apt-get install -y --allow-downgrades \"shaprint=\${STABLE_VERSION}\"
    wait_for_shaprintd

    CERT_SHA_DOWNGRADE=\$(sha256sum /var/lib/shaprint/server-identity.cert.der | awk '{print \$1}')
    if [[ \"\${CERT_SHA_STABLE}\" != \"\${CERT_SHA_DOWNGRADE}\" ]]; then
        echo 'error: apt downgrade replaced server identity' >&2
        exit 1
    fi
    shaprintctl status | grep -q '^network-channel: configured$'
    echo 'verified: APT channel switch Stable <-> Nightly preserves single package identity and state'

    # 6. Verify security: untrusted repository without valid signature is rejected
    rm -f /etc/apt/sources.list.d/shaprint.list
    echo 'deb file:///repos/stable/deb ./' > /etc/apt/sources.list.d/shaprint-untrusted.list
    if ! apt-get update 2>&1 | grep -qiE 'is not signed|no_pubkey'; then
        echo 'error: apt-get update must reject untrusted unsigned repository' >&2
        exit 1
    fi
    echo 'verified: APT rejects unsigned or unauthenticated repositories safely'
"

podman rm -f "${U24_CONTAINER}" >/dev/null

echo "All signed repository channel switch and verification tests passed successfully."
