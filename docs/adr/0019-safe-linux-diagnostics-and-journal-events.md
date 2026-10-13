# Safe Linux printing diagnostics and structured journal events

---
status: accepted
extends: ADR 0008 (print failure reporting), ADR 0018 (Linux system daemon)
---

Linux printing troubleshooting spans multiple operating system and network layers: the `shaprintd` daemon process, dedicated port listeners, the local CUPS spooler socket, TLS server trust entries, machine Network Channel credentials, and host firewall packet filters (`firewalld` / `ufw`). When printing fails or readiness is blocked, users and administrators require clear, actionable diagnostics across both CLI (`shaprintctl`) and GUI (`SystemDiagnosticsDialog`), as well as structured observability in the system journal (`journalctl -u shaprintd`).

## Zero disclosure of protected secrets

In accordance with ADR 0008 and ADR 0018, diagnostic reports, CLI text, GUI dialogs, IPC DTOs, Unix control socket payloads, and system journal events must never expose:
1. Network Channel values, verification tokens, or salted verifiers.
2. IPP / IPPS URIs containing credentials or embedded authorization keys.
3. Print document payloads, spool files, rendered raster streams, or driver byte buffers.

Subsystem checks and failure reports derive messages and recovery commands structurally from known subsystem identities, status codes, and validated public identifiers (e.g. queue names, local port numbers, trusted server addresses). There is no constructor or formatting path that accepts raw print payloads or secret credentials.

## Subsystem checks and concrete recovery guidance

`shaprintctl diagnostics` (and GUI `SystemDiagnosticsDialog`) evaluates six discrete subsystems:

1. **`daemon`**: Evaluates whether `shaprintd` is active and responsive over the local Unix domain socket (`/run/shaprint/control.sock`). Recovery action points to starting or enabling the systemd unit (`sudo systemctl start shaprintd`).
2. **`ports`**: Verifies availability and ownership of standard ShaPrint ports (TCP `48631` for IPPS server, loopback TCP `48632` for client proxy, UDP `48633` for discovery). Conflict detection inspects process ownership and advises terminating conflicting processes or stopping conflicting services.
3. **`cups`**: Validates availability of the local CUPS spooler (`/run/cups/cups.sock` or `localhost:631`) and inspects registered queues. If the socket is unreachable, diagnostics prescribes starting the CUPS service (`sudo systemctl start cups`).
4. **`trust`**: Inspects machine trust entries (`/var/lib/shaprint/trusted_servers.json`) for configured Client connections. If any server certificate has changed or is unapproved, diagnostics warns and provides the approval command (`shaprintctl trust approve <server>`).
5. **`credential`**: Checks whether the machine Network Channel is configured in protected storage (`/var/lib/shaprint/client_credential.json` / state directory). If missing, diagnostics guides configuration (`sudo shaprintctl channel set`).
6. **`firewall`**: Inspects active firewall managers (`firewalld` or `ufw`) to verify whether port `48631/tcp` (and `48633/udp`) are permitted for inbound server sharing. If blocked, concrete commands guide opening the ports (`sudo shaprintctl firewall open`).

## Unified semantics across CLI, GUI, and Journal

- **Readiness State**: Both CLI `shaprintctl status` and GUI reflect operational readiness as `ready` or `not-ready`. Readiness requires that the daemon is active, services are not failed, and the Network Channel is configured.
- **Diagnostics Formatting**: Subsystem results present human-readable status badges (`[OK]`, `[WARN]`, `[FAIL]`, `[SKIP]`), precise issue summaries, and concrete remediation actions.
- **Structured Journal Logging**: The daemon records structured key-value events to standard logger / system journal (`event=diagnostics_summary readiness=... healthy=... warning=... error=...`, `event=diagnostic_check subsystem=... status=...`). Messages adhere strictly to structural fields without format string interpolation of sensitive user data.

## Consequences and Compatibility

- **Stable ↔ Nightly Compatibility**: Unaffected. No changes to the wire protocol, dedicated ports (TCP `48631`, TCP `48632`, UDP `48633`), IPPS queue paths, or authentication schemes. Stable clients can print to Nightly servers and vice versa.
- **Settings & State Backward Compatibility**: Diagnostics queries live system state and existing persistent configuration (`client_credential.json`, `trusted_servers.json`, `config.json`). No breaking state schema migrations are introduced.
