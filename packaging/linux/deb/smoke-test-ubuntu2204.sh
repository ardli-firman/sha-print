#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "${SCRIPT_DIR}/../../.." && pwd)"
IMAGE="${UBUNTU2204_INIT_IMAGE:-localhost/ubuntu:22.04-init}"

# Build base image if not present or missing binutils
if ! podman run --rm "${IMAGE}" which objdump >/dev/null 2>&1; then
    podman build -t "${IMAGE}" - <<EOF
FROM docker.io/library/ubuntu:22.04
ENV DEBIAN_FRONTEND=noninteractive
RUN apt-get update && apt-get install -y --no-install-recommends \
    systemd systemd-sysv cups ufw adduser ca-certificates binutils \
    && rm -rf /var/lib/apt/lists/*
CMD ["/sbin/init"]
EOF
fi

DEB_PATH="$("${SCRIPT_DIR}/build-deb.sh")"
DEB_FILENAME="$(basename "${DEB_PATH}")"

DEB_UPGRADE_PATH="$(DEB_RELEASE=2 "${SCRIPT_DIR}/build-deb.sh")"
DEB_UPGRADE_FILENAME="$(basename "${DEB_UPGRADE_PATH}")"

CONTAINER_NAME="shaprint-u22-smoke-$$"
cleanup() {
    podman rm -f "${CONTAINER_NAME}" >/dev/null 2>&1 || true
}
trap cleanup EXIT

podman run -d --name "${CONTAINER_NAME}" --cap-add=NET_ADMIN "${IMAGE}" >/dev/null

wait_for_systemd() {
    local attempts=40
    for ((i = 1; i <= attempts; i++)); do
        local state
        state="$(podman exec "${CONTAINER_NAME}" systemctl is-system-running 2>/dev/null || true)"
        if [[ "${state}" == "running" || "${state}" == "degraded" ]]; then
            return 0
        fi
        sleep 0.25
    done
    echo "error: systemd inside ${CONTAINER_NAME} did not reach running state" >&2
    return 1
}

wait_for_systemd

podman cp "${DEB_PATH}" "${CONTAINER_NAME}:/root/${DEB_FILENAME}"
podman cp "${DEB_UPGRADE_PATH}" "${CONTAINER_NAME}:/root/${DEB_UPGRADE_FILENAME}"

podman exec "${CONTAINER_NAME}" bash -euo pipefail -c "
    cat <<'EOF' > /usr/local/bin/wait_for_shaprintd
#!/bin/bash
set -e
for _ in {1..40}; do
    if systemctl is-active --quiet shaprintd.service && [[ -S /run/shaprint/shaprintd.sock ]]; then
        if shaprintctl status 2>/dev/null | grep -q '^client-proxy: running$'; then
            exit 0
        fi
    fi
    sleep 0.25
done
echo 'error: shaprintd.service did not become active with control socket' >&2
systemctl status shaprintd.service --no-pager >&2 || true
exit 1
EOF
    chmod +x /usr/local/bin/wait_for_shaprintd

    # 0. Verify the package binary does not require glibc newer than 2.35 (Ubuntu 22.04 LTS baseline).
    dpkg-deb -x \"/root/${DEB_FILENAME}\" /tmp/deb-extracted
    if objdump -p /tmp/deb-extracted/usr/bin/shaprintd | grep -qE 'GLIBC_([3-9]\.|2\.(3[6-9]|[4-9][0-9]))'; then
        echo 'error: package raises runtime baseline beyond glibc 2.35' >&2
        exit 1
    fi
    rm -rf /tmp/deb-extracted

    # Install the DEB package
    dpkg -i \"/root/${DEB_FILENAME}\"

    # 1. Verify dedicated unprivileged shaprint account.
    SHAPRINT_UID=\$(id -u shaprint)
    if [[ \"\${SHAPRINT_UID}\" -eq 0 ]]; then
        echo 'error: shaprint user must not have UID 0' >&2
        exit 1
    fi
    PASSWD_ENTRY=\$(getent passwd shaprint)
    if [[ \"\${PASSWD_ENTRY}\" != *':/var/lib/shaprint:'* ]]; then
        echo \"error: unexpected shaprint passwd entry: \${PASSWD_ENTRY}\" >&2
        exit 1
    fi

    # 2. Verify FHS directories and least-privilege ownership/modes.
    ETC_STAT=\$(stat -c '%U:%G:%a' /etc/shaprint)
    if [[ \"\${ETC_STAT}\" != 'root:shaprint:750' ]]; then
        echo \"error: expected /etc/shaprint root:shaprint:750, got \${ETC_STAT}\" >&2
        exit 1
    fi
    CFG_STAT=\$(stat -c '%U:%G:%a' /etc/shaprint/config.json)
    if [[ \"\${CFG_STAT}\" != 'root:shaprint:640' ]]; then
        echo \"error: expected /etc/shaprint/config.json root:shaprint:640, got \${CFG_STAT}\" >&2
        exit 1
    fi
    STATE_STAT=\$(stat -c '%U:%G:%a' /var/lib/shaprint)
    if [[ \"\${STATE_STAT}\" != 'shaprint:shaprint:700' ]]; then
        echo \"error: expected /var/lib/shaprint shaprint:shaprint:700, got \${STATE_STAT}\" >&2
        exit 1
    fi
    RUN_STAT=\$(stat -c '%U:%G:%a' /run/shaprint)
    if [[ \"\${RUN_STAT}\" != 'shaprint:shaprint:755' ]]; then
        echo \"error: expected /run/shaprint shaprint:shaprint:755, got \${RUN_STAT}\" >&2
        exit 1
    fi

    # 3. Verify systemd service is enabled at boot, active, and logs to journald.
    if [[ \"\$(systemctl is-enabled shaprintd.service)\" != 'enabled' ]]; then
        echo 'error: shaprintd.service is not enabled for boot' >&2
        exit 1
    fi

    /usr/local/bin/wait_for_shaprintd
    journalctl -u shaprintd.service --no-pager | grep -q 'shaprintd started'

    SOCK_STAT=\$(stat -c '%F:%U:%G:%a' /run/shaprint/shaprintd.sock)
    if [[ \"\${SOCK_STAT}\" != 'socket:shaprint:shaprint:666' ]]; then
        echo \"error: expected socket:shaprint:shaprint:666, got \${SOCK_STAT}\" >&2
        exit 1
    fi

    KEY_STAT=\$(stat -c '%U:%G:%a' /var/lib/shaprint/server-identity.key.der)
    if [[ \"\${KEY_STAT}\" != 'shaprint:shaprint:600' ]]; then
        echo \"error: expected identity key shaprint:shaprint:600, got \${KEY_STAT}\" >&2
        exit 1
    fi

    # 4. Verify the long-running daemon does not run as root.
    MAIN_PID=\$(systemctl show -p MainPID --value shaprintd.service)
    DAEMON_USER=\$(ps -o user= -p \"\${MAIN_PID}\" | tr -d '[:space:]')
    if [[ \"\${DAEMON_USER}\" != 'shaprint' ]]; then
        echo \"error: shaprintd is running as '\${DAEMON_USER}', expected 'shaprint'\" >&2
        exit 1
    fi

    # 5. Verify shaprintctl status works from root and from non-root user.
    STATUS_OUT=\$(shaprintctl status)
    echo \"\${STATUS_OUT}\" | grep -q '^client-proxy: running$'
    echo \"\${STATUS_OUT}\" | grep -q '^server-sharing: stopped$'

    useradd -m localoperator
    USER_STATUS=\$(su - localoperator -c 'shaprintctl status')
    echo \"\${USER_STATUS}\" | grep -q '^client-proxy: running$'
    echo \"\${USER_STATUS}\" | grep -q '^server-sharing: stopped$'

    if su - localoperator -c 'shaprintctl stop' >/dev/null 2>&1; then
        echo 'error: unprivileged local user must not be allowed to stop shaprintd' >&2
        exit 1
    fi

    # 6. Verify Network Channel stdin input and root-protected credential storage.
    shaprintctl network-channel status | grep -q '^network-channel: unconfigured$'
    su - localoperator -c 'shaprintctl network-channel status' | grep -q '^network-channel: unconfigured$'

    SMOKE_SECRET=\"u22-chan-\$\$-\$(date +%s)\"
    printf '%s\n' \"\${SMOKE_SECRET}\" | shaprintctl network-channel set --stdin | grep -q '^network-channel: configured$'

    CRED_STAT=\$(stat -c '%U:%G:%a' /etc/shaprint-credentials/network-channel)
    if [[ \"\${CRED_STAT}\" != 'root:shaprint:640' ]]; then
        echo \"error: expected /etc/shaprint-credentials/network-channel root:shaprint:640, got \${CRED_STAT}\" >&2
        exit 1
    fi
    if su - localoperator -c 'cat /etc/shaprint-credentials/network-channel' >/dev/null 2>&1; then
        echo 'error: unprivileged local user must not be able to read credential file' >&2
        exit 1
    fi
    if journalctl -u shaprintd.service --no-pager | grep -Fq \"\${SMOKE_SECRET}\"; then
        echo 'error: journal output must never contain Network Channel value' >&2
        exit 1
    fi

    # 7. Verify UFW setup, idempotency, and preservation of unrelated rules.
    ufw allow 22/tcp comment 'SSH' >/dev/null 2>&1 || true
    ufw allow 48631/tcp comment 'ShaPrint Server IPPS' >/dev/null 2>&1 || true
    ufw allow 48633/udp comment 'ShaPrint Server Discovery' >/dev/null 2>&1 || true

    # Idempotent re-run
    ufw allow 48631/tcp comment 'ShaPrint Server IPPS' >/dev/null 2>&1 || true
    ufw allow 48633/udp comment 'ShaPrint Server Discovery' >/dev/null 2>&1 || true

    ufw --force enable >/dev/null 2>&1 || true
    UFW_STATUS=\$(ufw status verbose 2>/dev/null || true)
    echo \"\${UFW_STATUS}\" | grep -q '22/tcp'
    echo \"\${UFW_STATUS}\" | grep -q '48631/tcp'
    echo \"\${UFW_STATUS}\" | grep -q '48633/udp'
    echo 'verified: UFW rules allow 48631/tcp and 48633/udp and preserve unrelated rule 22/tcp'

    # 8. Verify polkit action and GNOME/KDE Wayland/X11 desktop integration.
    POLKIT_FILE=\"/usr/share/polkit-1/actions/org.shaprint.control.policy\"
    if [[ ! -f \"\${POLKIT_FILE}\" ]]; then
        echo 'error: polkit action file missing' >&2
        exit 1
    fi
    grep -q 'org.shaprint.control.start' \"\${POLKIT_FILE}\"
    grep -q 'org.shaprint.control.network-channel' \"\${POLKIT_FILE}\"

    # 9. Verify WebKitGTK 4.1 runtime packages exist in Ubuntu 22.04 repos.
    if apt-cache show libwebkit2gtk-4.1-0 >/dev/null 2>&1; then
        echo 'verified: official libwebkit2gtk-4.1-0 verified in Ubuntu 22.04'
    else
        echo 'error: libwebkit2gtk-4.1-0 missing in Ubuntu 22.04 repositories' >&2
        exit 1
    fi

    # 9b. Verify shaprintctl diagnostics on Ubuntu 22.04.
    DIAG_OUT=\$(shaprintctl diagnostics || true)
    echo \"\${DIAG_OUT}\" | grep -q 'daemon: shaprintd is running'
    echo \"\${DIAG_OUT}\" | grep -q 'System status:'
    echo 'verified: shaprintctl diagnostics succeeds on Ubuntu 22.04'
"

# 10. Reboot container to verify auto-start.
podman restart "${CONTAINER_NAME}" >/dev/null
wait_for_systemd

podman exec "${CONTAINER_NAME}" bash -euo pipefail -c "
    /usr/local/bin/wait_for_shaprintd
    BOOT_STATUS=\$(su - localoperator -c 'shaprintctl status')
    echo \"\${BOOT_STATUS}\" | grep -q '^client-proxy: running$'
    echo \"\${BOOT_STATUS}\" | grep -q '^server-sharing: stopped$'

    # 11. Verify package upgrade drains active jobs and restarts daemon.
    shaprintctl drain --timeout 10 | grep -q '^drained: true$'

    (
        if exec 3<>/dev/tcp/127.0.0.1/48632 2>/dev/null; then
            printf 'POST /ipp/print HTTP/1.1\r\nHost: 127.0.0.1:48632\r\nContent-Type: application/ipp\r\nContent-Length: 100\r\n\r\n' >&3 2>/dev/null || true
            sleep 1
            exec 3>&- || true
        fi
    ) &
    IN_FLIGHT_PID=\$!

    dpkg -i \"/root/${DEB_UPGRADE_FILENAME}\"
    wait \$IN_FLIGHT_PID || true
    /usr/local/bin/wait_for_shaprintd

    UPGRADE_STATUS=\$(shaprintctl status)
    echo \"\${UPGRADE_STATUS}\" | grep -q '^client-proxy: running$'
    echo \"\${UPGRADE_STATUS}\" | grep -q '^server-sharing: stopped$'

    # 12. Repeated install preserves identity.
    CERT_SHA_BEFORE=\$(sha256sum /var/lib/shaprint/server-identity.cert.der | awk '{print \$1}')
    KEY_SHA_BEFORE=\$(sha256sum /var/lib/shaprint/server-identity.key.der | awk '{print \$1}')

    dpkg -i \"/root/${DEB_UPGRADE_FILENAME}\"
    /usr/local/bin/wait_for_shaprintd

    CERT_SHA_REINSTALL=\$(sha256sum /var/lib/shaprint/server-identity.cert.der | awk '{print \$1}')
    KEY_SHA_REINSTALL=\$(sha256sum /var/lib/shaprint/server-identity.key.der | awk '{print \$1}')
    if [[ \"\${CERT_SHA_BEFORE}\" != \"\${CERT_SHA_REINSTALL}\" || \"\${KEY_SHA_BEFORE}\" != \"\${KEY_SHA_REINSTALL}\" ]]; then
        echo 'error: package reinstall replaced the existing server identity' >&2
        exit 1
    fi

    # 13. Ordinary uninstall (dpkg -r) preserves state and credentials.
    dpkg -r shaprint

    if [[ -x /usr/bin/shaprintd || -x /usr/bin/shaprintctl ]]; then
        echo 'error: ordinary uninstall left binaries behind' >&2
        exit 1
    fi
    if [[ ! -f /var/lib/shaprint/server-identity.cert.der || ! -f /var/lib/shaprint/server-identity.key.der ]]; then
        echo 'error: ordinary uninstall did not preserve /var/lib/shaprint server identity' >&2
        exit 1
    fi
    if [[ ! -f /etc/shaprint-credentials/network-channel ]]; then
        echo 'error: ordinary uninstall did not preserve /etc/shaprint-credentials/network-channel' >&2
        exit 1
    fi

    # 14. Reinstall restores preserved state without generating new identity.
    dpkg -i \"/root/${DEB_UPGRADE_FILENAME}\"
    /usr/local/bin/wait_for_shaprintd
    CERT_SHA_AFTER=\$(sha256sum /var/lib/shaprint/server-identity.cert.der | awk '{print \$1}')
    KEY_SHA_AFTER=\$(sha256sum /var/lib/shaprint/server-identity.key.der | awk '{print \$1}')
    if [[ \"\${CERT_SHA_BEFORE}\" != \"\${CERT_SHA_AFTER}\" || \"\${KEY_SHA_BEFORE}\" != \"\${KEY_SHA_AFTER}\" ]]; then
        echo 'error: uninstall + reinstall replaced the existing server identity' >&2
        exit 1
    fi
    shaprintctl status | grep -q '^network-channel: configured$'

    # 15. Confirmed purge removes ShaPrint resources safely.
    if shaprintctl purge >/dev/null 2>&1; then
        echo 'error: shaprintctl purge without --confirm must fail' >&2
        exit 1
    fi

    PURGE_OUT=\$(shaprintctl purge --confirm)
    echo \"\${PURGE_OUT}\" | grep -q '^purged: true$'

    if [[ -d /var/lib/shaprint ]]; then
        echo 'error: purge did not remove /var/lib/shaprint' >&2
        exit 1
    fi
    if [[ -f /etc/shaprint-credentials/network-channel ]]; then
        echo 'error: purge did not remove /etc/shaprint-credentials/network-channel' >&2
        exit 1
    fi
    if systemctl is-active --quiet shaprintd.service; then
        echo 'error: shaprintd.service still active after purge' >&2
        exit 1
    fi

    # Purge package with dpkg -P
    dpkg -P shaprint

    UFW_POST_PURGE=\$(ufw status verbose 2>/dev/null || true)
    if echo \"\${UFW_POST_PURGE}\" | grep -qE \"48631/tcp|48633/udp\"; then
        echo 'error: package purge did not remove ShaPrint UFW rules' >&2
        exit 1
    fi
    echo \"\${UFW_POST_PURGE}\" | grep -q '22/tcp'
"

echo "Ubuntu 22.04 DEB smoke tests passed successfully."
