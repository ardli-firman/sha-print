# IPPS sharing endpoint and server identity

---
status: accepted
---

The Windows server exposes the printer queues the user selected through one Rust endpoint that
speaks IPP over HTTP over TLS ("IPPS"). The MVP endpoint answers queries only: `Get-Printers`
(0x0402) and `Get-Printer-Attributes` (0x000B). Job submission arrives with #32, so every other
operation is rejected with `server-error-operation-not-supported` instead of half-working.

The endpoint listens on TCP 8631 by default, not 631: Windows' own IPP service owns 631 when the
Internet Printing feature is installed, and ShaPrint must not compete for it. It binds while server
sharing runs and closes the listener when sharing stops, so a stopped server refuses new client
connections. Every query answers from the current selection, so changing the shared queues takes
effect without restarting sharing.

The server's identity is a self-signed certificate generated once and kept in the app data
directory (`server-identity.cert.der` and `server-identity.key.der`, the key restricted to the
current user where the platform supports it). Clients approve its SHA-256 fingerprint on first use,
so the identity must stay stable across runs; the shell reports a half-written identity instead of
replacing it. The fingerprint is shown to the server user and travels over IPC, but the private key
never leaves the endpoint adapter.

Only inbound access needs administrator rights: Windows blocks inbound connections to a program
until a firewall rule allows them. The app asks for a UAC prompt for that one action and re-runs its
own executable with `--setup allow-inbound-sharing`, which adds the rule. Selecting queues, starting
and stopping sharing, and showing the fingerprint are per-user actions and never prompt.

## Consequences

- Clients need the server's address and port; mDNS advertisement is #36, manual entry is #33.
- Clients must reach the port through the Windows firewall, so the first client connection from
  another computer fails until the server user grants inbound access.
- The endpoint binds the IPv4 wildcard address. Dual-stack binding and the later Linux phase's CUPS
  endpoint are separate adapters behind the same ports.
- The identity and its fingerprint survive restarts, which is what makes "a changed fingerprint
  needs attention" meaningful for clients (#33).
- HTTPS framing is minimal on purpose: one POST with `Content-Type: application/ipp` and a
  bounded body. Chunked request bodies and other methods are refused rather than misread.
