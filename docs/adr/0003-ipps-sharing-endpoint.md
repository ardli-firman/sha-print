# IPPS sharing endpoint and server identity

---
status: accepted
---

The Windows server exposes the printer queues the user selected through one Rust endpoint that
speaks IPP over HTTP over TLS ("IPPS"). While sharing runs, it answers `Get-Printers` (0x0402) and
`Get-Printer-Attributes` (0x000B), and accepts `Print-Job` (0x0002) only when:

- a Network Channel is configured and the request's `network-channel` operation attribute matches;
- the requested printer is in the current shared-queue selection; and
- a platform print-job adapter is available.

Missing or incorrect channels and unshared queues never reach the printer adapter. The configured
Network Channel is replaced through an unprivileged UI action. App data retains a SHA-256 verifier
and a fresh random 128-bit salt for each configuration; hashing `salt || channel` prevents
precomputed rainbow-table attacks without imposing a slow password-KDF cost on each print request.
This does not prevent targeted offline guessing of a low-entropy Network Channel. Older unsalted
verifier files are treated as unconfigured and require the user to set the Network Channel again.
The channel value is never returned through IPC or written to logs, status, or responses.

The endpoint listens on TCP 8631 by default, not 631: Windows' own IPP service owns 631 when the
Internet Printing feature is installed, and ShaPrint must not compete for it. It binds while server
sharing runs and closes the listener when sharing stops, so a stopped server refuses new client
connections. Every query and print job uses the current selection, so changing shared queues takes
effect without restarting sharing.

Responses follow RFC 8010: the status code occupies the header field a request uses for its
operation id, and each printer is reported as its own attributes group identified by
`printer-uri-supported`. The endpoint advertises only `application/octet-stream` as
`document-format-supported`; clients must provide printer-ready spool data, while other formats are
rejected with `client-error-document-format-not-supported` before submission. Printer status reports
whether the channel and job-submission adapter are available. A successful `Print-Job` response
includes the spooler's job id, job URI, and pending state. Common `media`, `print-color-mode`,
`sides`, and `copies` values are mapped to the Windows queue's DEVMODE; unsupported values are
rejected before the document is submitted.

The server's identity is a self-signed certificate generated once and kept in the app data
directory (`server-identity.cert.der` and `server-identity.key.der`, the key restricted to the
current user where the platform supports it). Clients can enter a host or host:port manually. The
first inspection opens TLS only and displays the certificate's SHA-256 fingerprint; it sends no IPP
query until the user explicitly approves that identity. Approval persists per normalized address.
Each printer query checks the live fingerprint against the saved approval, and a changed identity
blocks the query until the user deliberately reapproves the newly reviewed fingerprint. The shell
reports a half-written server identity instead of replacing it. Its private key never leaves the
endpoint adapter.

Only inbound access needs administrator rights: Windows blocks inbound connections to a program
until a firewall rule allows them. The app asks for a UAC prompt for that one action and re-runs its
own executable with `--setup allow-inbound-sharing`, which adds the rule. Queue selection, sharing
start/stop, fingerprint review, and Network Channel configuration are per-user actions and never
prompt.

## Consequences

- Clients can enter the server's address and port manually; mDNS advertisement remains #36.
- Clients must reach the port through the Windows firewall, so the first client connection from
  another computer fails until the server user grants inbound access.
- The endpoint binds the IPv4 wildcard address. Dual-stack binding and the later Linux phase's CUPS
  endpoint are separate adapters behind the same ports.
- The identity and its fingerprint survive restarts, which is what makes "a changed fingerprint
  needs attention" meaningful for clients (#33).
- HTTPS framing is minimal on purpose: one POST with `Content-Type: application/ipp` and a
  bounded body. Chunked request bodies and other methods are refused rather than misread.
