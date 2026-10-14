%global debug_package %{nil}
%global _build_id_links none

Name:           shaprint
Version:        %{_shaprint_version}
Release:        %{?_shaprint_release}%{!?_shaprint_release:1}%{?dist}
Summary:        ShaPrint headless print-sharing and client-proxy daemon
License:        MIT
URL:            https://github.com/ardli-firman/sha-print
ExclusiveArch:  x86_64

Requires(pre):  shadow-utils
Requires:       systemd
Requires:       cups

%description
ShaPrint shares printers over LAN and cross-VLAN using standard IPPS and
multicast DNS discovery. This package installs the machine-wide shaprintd
daemon, shaprintctl CLI, dedicated unprivileged shaprint account, and
systemd service integration.

%install
install -D -m 0755 "%{_sourcedir}/shaprintd" "%{buildroot}%{_bindir}/shaprintd"
install -D -m 0755 "%{_sourcedir}/shaprintctl" "%{buildroot}%{_bindir}/shaprintctl"
install -D -m 0644 "%{_sourcedir}/shaprintd.service" "%{buildroot}%{_unitdir}/shaprintd.service"
install -D -m 0644 "%{_sourcedir}/shaprint.sysusers" "%{buildroot}%{_sysusersdir}/shaprint.conf"
install -D -m 0644 "%{_sourcedir}/shaprint.tmpfiles" "%{buildroot}%{_tmpfilesdir}/shaprint.conf"
install -D -m 0644 "%{_sourcedir}/90-shaprint.preset" "%{buildroot}%{_presetdir}/90-shaprint.preset"
install -d -m 0750 "%{buildroot}%{_sysconfdir}/shaprint"
install -d -m 0750 "%{buildroot}%{_sysconfdir}/shaprint-credentials"
install -D -m 0640 "%{_sourcedir}/config.json" "%{buildroot}%{_sysconfdir}/shaprint/config.json"
# Narrow polkit actions for the desktop GUI's `pkexec shaprintctl` machine mutations (ADR 0018).
install -D -m 0644 "%{_sourcedir}/org.shaprint.control.policy" "%{buildroot}%{_datadir}/polkit-1/actions/org.shaprint.control.policy"
install -d -m 0700 "%{buildroot}%{_sharedstatedir}/shaprint"

%pre
getent group shaprint >/dev/null 2>&1 || groupadd -r shaprint
getent passwd shaprint >/dev/null 2>&1 || \
    useradd -r -g shaprint -d /var/lib/shaprint -s /sbin/nologin \
    -c "ShaPrint system daemon" shaprint
for grp in sys lp lpadmin; do
    if getent group "$grp" >/dev/null 2>&1; then
        usermod -a -G "$grp" shaprint 2>/dev/null || :
    fi
done
if [ "$1" -ge 2 ]; then
    if [ -x %{_bindir}/shaprintctl ] && [ -S /run/shaprint/shaprintd.sock ]; then
        # Try graceful drain if supported by installed version; do not block upgrade if older binary lacks drain
        %{_bindir}/shaprintctl drain --timeout 30 >/dev/null 2>&1 || :
    fi
fi
exit 0

%post
systemd-tmpfiles --create %{_tmpfilesdir}/shaprint.conf >/dev/null 2>&1 || :
systemctl daemon-reload >/dev/null 2>&1 || :
if [ "$1" -eq 1 ]; then
    systemctl enable shaprintd.service >/dev/null 2>&1 || :
    if [ -d /run/systemd/system ]; then
        systemctl start shaprintd.service >/dev/null 2>&1 || :
    fi
elif [ -d /run/systemd/system ]; then
    systemctl try-restart shaprintd.service >/dev/null 2>&1 || :
fi
exit 0

%preun
if [ "$1" -eq 0 ]; then
    if [ -d /run/systemd/system ]; then
        systemctl --no-reload disable --now shaprintd.service >/dev/null 2>&1 || :
    else
        systemctl disable shaprintd.service >/dev/null 2>&1 || :
    fi
fi
exit 0

%postun
systemctl daemon-reload >/dev/null 2>&1 || :
exit 0

%files
%attr(0755,root,root) %{_bindir}/shaprintd
%attr(0755,root,root) %{_bindir}/shaprintctl
%attr(0644,root,root) %{_unitdir}/shaprintd.service
%attr(0644,root,root) %{_sysusersdir}/shaprint.conf
%attr(0644,root,root) %{_tmpfilesdir}/shaprint.conf
%attr(0644,root,root) %{_presetdir}/90-shaprint.preset
%dir %attr(0750,root,shaprint) %{_sysconfdir}/shaprint
%dir %attr(0750,root,shaprint) %{_sysconfdir}/shaprint-credentials
%config(noreplace) %attr(0640,root,shaprint) %{_sysconfdir}/shaprint/config.json
%dir %attr(0700,shaprint,shaprint) %{_sharedstatedir}/shaprint
%attr(0644,root,root) %{_datadir}/polkit-1/actions/org.shaprint.control.policy

%changelog
* Wed Oct 14 2026 ShaPrint Maintainers <maintainers@shaprint.local> - 3.4.0-1
- Extend headless daemon RPM package support to AlmaLinux 9 and AlmaLinux 10

* Tue Oct 13 2026 ShaPrint Maintainers <maintainers@shaprint.local> - 3.4.0-1
- Deliver AlmaLinux 10 headless daemon RPM package baseline

