# Linux support through a system daemon and CUPS adapters

---
status: accepted
extends: ADR 0001 (Linux phase), ADR 0002 (platform adapters), ADR 0014 (fixed ports)
supersedes: direct-CUPS authentication details for Linux in ADR 0001
---

ShaPrint will support Ubuntu, Debian, and AlmaLinux as both Client and Server without creating a second wire protocol. Linux keeps the existing Rust IPPS endpoint, loopback client proxy, Network Channel authentication, certificate-fingerprint trust, printer route, discovery schema, and fixed TCP `48631`, loopback TCP `48632`, and UDP `48633` ports. CUPS sits behind the existing platform adapter seams: it supplies native Client queues, local printer discovery, and print-job submission, but it is not exposed as ShaPrint's network Server. This preserves Windows Stable ↔ Linux Nightly printing in both directions and avoids making direct CUPS authentication or Avahi discovery a separate compatibility path.

One machine-wide `shaprintd` service will own the Client proxy and Server runtime under a dedicated unprivileged `shaprint` account. The runtimes remain independently controllable: the configured Client proxy starts at boot, while Server sharing restores its previous Start/Stop state. Client queues, Trusted Servers, and Client credentials are machine state; the Network Channel is held in root-protected system credential storage, while Server authorization retains only its salted verifier. The GUI and the fully capable `shaprintctl` CLI use a local Unix socket. Reads expose no secret or print content, mutations require polkit from the desktop or `sudo` from the CLI, and remote administration uses SSH rather than a network admin API.

Linux state follows the filesystem hierarchy: non-secret configuration in `/etc/shaprint`, identity and durable state in `/var/lib/shaprint`, the control socket in `/run/shaprint`, and secret-free diagnostics in the system journal. The desktop targets GNOME and KDE on Wayland and X11; closing it does not stop the daemon, and tray integration is optional. The first release target is IPv4 on `x86_64`. Host setup automates firewalld and UFW, uses stable CUPS-safe identifiers plus human-readable descriptions for Client queues, and never takes over a conflicting queue it does not own.

ShaPrint will publish signed `.rpm` packages for AlmaLinux and `.deb` packages for Ubuntu and Debian through separate Stable and Nightly repository channels. Both channels use one package identity and shared state, so they replace rather than coexist with each other. AppImage may be evaluated later as an optional GUI artifact, not as the supported system-integration path. Package updates drain active print jobs with a bounded timeout before restarting the daemon. Uninstall preserves state and queues for reinstall or upgrade; an explicit confirmed purge removes ShaPrint-owned queues, credentials, identity, state, service, and firewall rules without deleting local printer queues.

## Support gate

The target matrix is two validated Ubuntu LTS releases, two Debian Stable releases, and two AlmaLinux major releases. A matrix entry is supported only after package install/upgrade/remove, daemon lifecycle, CUPS integration, firewalld or UFW setup, GNOME/KDE GUI smoke, and every Linux ↔ Windows Client/Server combination pass. Representative physical smoke checks must cover a driverless printer and a USB or local CUPS queue; successful spooling alone is insufficient. Printing must preserve visual output and requested media, color, duplex, orientation, and copies relative to printing directly through the same CUPS queue. A distribution leaving the matrix receives one Stable release of notice before new builds stop.

Linux Client work may land in Nightly before Server work, but Stable will not claim Linux support until both roles and the interoperability matrix pass. Linux-specific state must be additive and readable after downgrading to Stable; any unavoidable persisted-state incompatibility requires a breaking-change release marker and `SETTINGS_RISK=true` for the affected Nightly.
