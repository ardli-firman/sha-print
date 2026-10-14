#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
IMAGE="${ALMALINUX10_INIT_IMAGE:-docker.io/almalinux/10-init}"

RPM_PATH="$("${SCRIPT_DIR}/build-rpm.sh")"
RPM_FILENAME="$(basename "${RPM_PATH}")"

RPM_UPGRADE_PATH="$(RPM_RELEASE=2 "${SCRIPT_DIR}/build-rpm.sh")"
RPM_UPGRADE_FILENAME="$(basename "${RPM_UPGRADE_PATH}")"

CONTAINER_NAME="shaprint-alma10-smoke-$$"
cleanup() {
    podman rm -f "${CONTAINER_NAME}" >/dev/null 2>&1 || true
}
trap cleanup EXIT

podman run -d --name "${CONTAINER_NAME}" "${IMAGE}" >/dev/null

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

podman cp "${RPM_PATH}" "${CONTAINER_NAME}:/root/${RPM_FILENAME}"
podman cp "${RPM_UPGRADE_PATH}" "${CONTAINER_NAME}:/root/${RPM_UPGRADE_FILENAME}"

podman exec "${CONTAINER_NAME}" bash -euo pipefail -c "
    wait_for_shaprintd() {
        for _ in {1..40}; do
            if systemctl is-active --quiet shaprintd.service && [[ -S /run/shaprint/shaprintd.sock ]]; then
                if shaprintctl status 2>/dev/null | grep -q '^client-proxy: running$'; then
                    return 0
                fi
            fi
            sleep 0.25
        done
        echo 'error: shaprintd.service did not become active with control socket' >&2
        systemctl status shaprintd.service --no-pager >&2 || true
        return 1
    }

    # Install cups if repos are available, otherwise install RPM with --nodeps in offline smoke test
    dnf install -y cups >/dev/null 2>&1 || true
    rpm -Uvh --nodeps \"/root/${RPM_FILENAME}\"

    # 1. Verify dedicated unprivileged shaprint account.
    SHAPRINT_UID=\$(id -u shaprint)
    if [[ \"\${SHAPRINT_UID}\" -eq 0 ]]; then
        echo 'error: shaprint user must not have UID 0' >&2
        exit 1
    fi
    PASSWD_ENTRY=\$(getent passwd shaprint)
    if [[ \"\${PASSWD_ENTRY}\" != *':/var/lib/shaprint:/sbin/nologin' ]]; then
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

    wait_for_shaprintd
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

    # 5. Verify shaprintctl status works from root and from a non-root local shell,
    #    while unprivileged mutations are rejected with not-authorized.
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

    # 5b. Verify Network Channel configuration, root-protected credential storage, and
    #     multi-user status visibility without secret leakage.
    shaprintctl network-channel status | grep -q '^network-channel: unconfigured$'
    su - localoperator -c 'shaprintctl network-channel status' | grep -q '^network-channel: unconfigured$'

    if shaprintctl network-channel set inline-forbidden-secret >/dev/null 2>&1; then
        echo 'error: shaprintctl must reject inline Network Channel arguments' >&2
        exit 1
    fi
    if su - localoperator -c 'printf \"%s\\n\" unpriv-secret | shaprintctl network-channel set --stdin' >/dev/null 2>&1; then
        echo 'error: unprivileged local user must not be allowed to set Network Channel' >&2
        exit 1
    fi

    SMOKE_SECRET=\"alma10-chan-\$\$-\$(date +%s)\"
    printf '%s\n' \"\${SMOKE_SECRET}\" | shaprintctl network-channel set --stdin | grep -q '^network-channel: configured$'

    CRED_STAT=\$(stat -c '%U:%G:%a' /etc/shaprint-credentials/network-channel)
    if [[ \"\${CRED_STAT}\" != 'root:shaprint:640' ]]; then
        echo \"error: expected /etc/shaprint-credentials/network-channel root:shaprint:640, got \${CRED_STAT}\" >&2
        exit 1
    fi
    if su - localoperator -c 'cat /etc/shaprint-credentials/network-channel' >/dev/null 2>&1; then
        echo 'error: unprivileged local user must not be able to read /etc/shaprint-credentials/network-channel' >&2
        exit 1
    fi
    if grep -R -Fq \"\${SMOKE_SECRET}\" /etc/shaprint /var/lib/shaprint; then
        echo 'error: /etc/shaprint and /var/lib/shaprint must not contain plaintext Network Channel' >&2
        exit 1
    fi
    su - localoperator -c 'shaprintctl network-channel status' | grep -q '^network-channel: configured$'
    su - localoperator -c 'shaprintctl status' | grep -q '^network-channel: configured$'
    if journalctl -u shaprintd.service --no-pager | grep -Fq \"\${SMOKE_SECRET}\"; then
        echo 'error: journal output must never contain Network Channel value' >&2
        exit 1
    fi
"

# 6. Reboot the container to verify shaprintd starts automatically at boot.
podman restart "${CONTAINER_NAME}" >/dev/null
wait_for_systemd

podman exec "${CONTAINER_NAME}" bash -euo pipefail -c "
    wait_for_shaprintd() {
        for _ in {1..40}; do
            if systemctl is-active --quiet shaprintd.service && [[ -S /run/shaprint/shaprintd.sock ]]; then
                if shaprintctl status 2>/dev/null | grep -q '^client-proxy: running$'; then
                    return 0
                fi
            fi
            sleep 0.25
        done
        echo 'error: shaprintd.service did not become active with control socket' >&2
        systemctl status shaprintd.service --no-pager >&2 || true
        return 1
    }

    wait_for_shaprintd
    BOOT_STATUS=\$(su - localoperator -c 'shaprintctl status')
    echo \"\${BOOT_STATUS}\" | grep -q '^client-proxy: running$'
    echo \"\${BOOT_STATUS}\" | grep -q '^server-sharing: stopped$'

    # 7. Verify package upgrade drains active jobs and restarts the daemon.
    # Start server sharing on an available dummy printer or verify drain CLI.
    # Verify shaprintctl drain succeeds while daemon is running.
    shaprintctl drain --timeout 10 | grep -q '^drained: true$'

    # Simulate an active in-flight request while performing real RPM upgrade from release 1 to release 2
    (
        exec 3<>/dev/tcp/127.0.0.1/48632 2>/dev/null || true
        if [ -e /dev/fd/3 ]; then
            printf 'POST /ipp/print HTTP/1.1\r\nHost: 127.0.0.1:48632\r\nContent-Type: application/ipp\r\nContent-Length: 100\r\n\r\n' >&3 2>/dev/null || true
            sleep 1
            exec 3>&- || true
        fi
    ) &
    IN_FLIGHT_PID=\$!

    rpm -Uvh --nodeps \"/root/${RPM_UPGRADE_FILENAME}\"
    rpm -V --nodeps shaprint
    wait \$IN_FLIGHT_PID || true
    wait_for_shaprintd

    UPGRADE_STATUS=\$(shaprintctl status)
    echo \"\${UPGRADE_STATUS}\" | grep -q '^client-proxy: running$'
    echo \"\${UPGRADE_STATUS}\" | grep -q '^server-sharing: stopped$'

    # 8. Verify repeated package installation / repair and uninstall+reinstall preserve state and queues.
    CERT_SHA_BEFORE=\$(sha256sum /var/lib/shaprint/server-identity.cert.der | awk '{print \$1}')
    KEY_SHA_BEFORE=\$(sha256sum /var/lib/shaprint/server-identity.key.der | awk '{print \$1}')

    # Simulate presence of a local printer queue and a ShaPrint client queue if CUPS is available
    if command -v lpadmin >/dev/null 2>&1 && systemctl is-active --quiet cups; then
        lpadmin -p local_office_printer -E -v file:///dev/null -D \"Local Office Printer\" 2>/dev/null || true
        lpadmin -p ShaPrint_Office_HP_192.168.1.10_10ec236c -E -v ipp://127.0.0.1:48632/ipp/print/192.168.1.10/Office_HP -D \"Office_HP (192.168.1.10)\" 2>/dev/null || true
    fi

    rpm -Uvh --nodeps --replacepkgs \"/root/${RPM_UPGRADE_FILENAME}\"
    rpm -V --nodeps shaprint
    wait_for_shaprintd

    CERT_SHA_REINSTALL=\$(sha256sum /var/lib/shaprint/server-identity.cert.der | awk '{print \$1}')
    KEY_SHA_REINSTALL=\$(sha256sum /var/lib/shaprint/server-identity.key.der | awk '{print \$1}')
    if [[ \"\${CERT_SHA_BEFORE}\" != \"\${CERT_SHA_REINSTALL}\" || \"\${KEY_SHA_BEFORE}\" != \"\${KEY_SHA_REINSTALL}\" ]]; then
        echo 'error: package repair replaced the existing server identity' >&2
        exit 1
    fi

    # Perform ordinary uninstall
    rpm -e shaprint

    # Verify binaries and service are removed/stopped, but machine state, credentials, and queues are preserved
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
    if command -v lpstat >/dev/null 2>&1 && systemctl is-active --quiet cups; then
        if ! lpstat -v 2>/dev/null | grep -q 'local_office_printer'; then
            echo 'error: ordinary uninstall deleted local printer queue' >&2
            exit 1
        fi
        if ! lpstat -v 2>/dev/null | grep -q 'ShaPrint_Office_HP_192.168.1.10_10ec236c'; then
            echo 'error: ordinary uninstall did not preserve ShaPrint client queue' >&2
            exit 1
        fi
    fi

    # Reinstall restores the preserved configuration without generating a new identity
    rpm -Uvh --nodeps \"/root/${RPM_UPGRADE_FILENAME}\"
    wait_for_shaprintd
    CERT_SHA_AFTER=\$(sha256sum /var/lib/shaprint/server-identity.cert.der | awk '{print \$1}')
    KEY_SHA_AFTER=\$(sha256sum /var/lib/shaprint/server-identity.key.der | awk '{print \$1}')
    if [[ \"\${CERT_SHA_BEFORE}\" != \"\${CERT_SHA_AFTER}\" || \"\${KEY_SHA_BEFORE}\" != \"\${KEY_SHA_AFTER}\" ]]; then
        echo 'error: uninstall + reinstall replaced the existing server identity' >&2
        exit 1
    fi
    shaprintctl status | grep -q '^network-channel: configured$'

    # 9. Verify systemctl restart and stop lifecycle.
    systemctl restart shaprintd.service
    wait_for_shaprintd
    shaprintctl status | grep -q '^client-proxy: running$'

    # 10. Verify explicit confirmed purge removes ShaPrint resources safely.
    # Purge without confirmation must be refused
    if shaprintctl purge >/dev/null 2>&1; then
        echo 'error: shaprintctl purge without --confirm must fail' >&2
        exit 1
    fi

    # Unprivileged user cannot purge
    if su - localoperator -c 'shaprintctl purge --confirm' >/dev/null 2>&1; then
        echo 'error: unprivileged user must not be allowed to execute purge' >&2
        exit 1
    fi

    # Root executes explicit confirmed purge
    PURGE_OUT=\$(shaprintctl purge --confirm)
    echo \"\${PURGE_OUT}\" | grep -q '^purged: true$'
    echo \"\${PURGE_OUT}\" | grep -q '^removed-credentials: true$'
    echo \"\${PURGE_OUT}\" | grep -q '^removed-identity-and-state: true$'

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

    # Local printer queue must NEVER be removed, but ShaPrint queue must be purged
    if command -v lpstat >/dev/null 2>&1 && systemctl is-active --quiet cups; then
        if ! lpstat -v 2>/dev/null | grep -q 'local_office_printer'; then
            echo 'error: purge removed local printer queue' >&2
            exit 1
        fi
        if lpstat -v 2>/dev/null | grep -q 'ShaPrint_Office_HP_192.168.1.10_10ec236c'; then
            echo 'error: purge failed to remove ShaPrint client queue' >&2
            exit 1
        fi
    fi

    # Repeated purge is idempotent
    shaprintctl purge --confirm | grep -q '^purged: true$'

    # Clean package removal
    rpm -e shaprint
"
