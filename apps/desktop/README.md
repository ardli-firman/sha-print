# ShaPrint desktop app

Tauri shell for the Windows-only IPP print-sharing MVP (issues #29–#41). It replaces the WPF
application as the product path and lives beside the .NET projects while the transition lasts.

## What exists today

- A Tauri window rendered by React + TypeScript, with one modular Rust crate in `src-tauri`.
- A runtime coordinator that owns the client proxy and server sharing runtimes: it starts them,
  publishes their live status to the window, and stops them cleanly when the app quits.
- Lifecycle: the app registers itself for the current user's Windows login (per-user `Run` key, no
  elevation), starts without a window from that launch, and closing its window hides it to the
  notification area. An explicit **Quit ShaPrint** in the tray menu stops the background services and
  exits; server sharing still only starts when the user starts it. The window turns login startup on
  or off (ADR 0005).
- Print problems: one surface explains the latest failed job with a stable code, a message derived
  from that code and the queue, and the single action that resolves it. A record holds no field a
  Network Channel value or document could travel in, the server reports only failures observed after
  authorization, and a print path that is not running names its recovery action beside Start
  (ADR 0007).
- Moving from the previous .NET app: the import reads `%LOCALAPPDATA%\ShaPrint` read-only and takes
  the Network Channel where the user set one. Printer queues are selected again in this app, no
  certificate approval is imported, and a second start changes nothing (ADR 0006).
- Server sharing: the user selects local Windows printer queues and starts or stops sharing
  explicitly. The IPPS endpoint answers `Get-Printers` and `Get-Printer-Attributes`, and accepts
  `Print-Job` only for a selected shared queue when the configured Network Channel matches.
- Windows spooler submission preserves common media size, color, duplex, and copy settings. A fake
  adapter exercises the real TLS/IPPS request path without printing during integration tests.
- The server identity's SHA-256 fingerprint stays stable in app data. A client can manually enter a
  host or host:port, inspect the presented fingerprint without querying printers, explicitly approve
  it, and list printers only while the live fingerprint matches the saved approval. Changed
  fingerprints remain blocked until explicit reapproval.
- The client proxy listens on `127.0.0.1:8632`. A manually configured IPP queue can route through it
  with `ipp://127.0.0.1:8632/ipp/print/{encoded-server-host:port}/{encoded-printer-name}`. The proxy
  replaces the local printer URI with the approved server's IPPS URI and supplies the Network
  Channel. Windows stores that client credential with DPAPI; the salted verifier remains the
  authorization form used by server sharing.
- Network Channel storage retains a salted SHA-256 verifier with a fresh 128-bit salt per
  configuration. Older unsalted verifier files are treated as unconfigured and require the user to
  set the Network Channel again. The plaintext value is never returned through IPC or written to
  logs, status, or UI after configuration.
- Discovery: while sharing runs, the server advertises its shared queues over multicast DNS, and a
  client browses for nearby servers and lists them live. Acting on a discovered server only fills in
  the address and starts the same certificate review; a discovered server is never trusted on the
  strength of its advertisement, and manual address entry keeps working where discovery cannot reach.
- One elevated setup action: letting clients reach the endpoint through the Windows firewall. Every
  other action — selecting queues, start/stop, fingerprint review, discovery, channel configuration,
  login startup, and the import from the previous app — runs unprivileged.
- Typed IPC: commands and status events use serializable DTOs, and failures carry stable codes
  (`invalid-input`, `unknown-service`, `invalid-state`, `timeout`, `unsupported`,
  `server-unavailable`, `server-untrusted`, `server-identity-changed`, `not-authorized`,
  `printer-not-shared`, `queue-unavailable`, `internal`).
- Least-privilege capabilities: the main window may call the shell's commands and listen for status
  events, and nothing else (no shell, filesystem, dialog, or remote content access).

Remaining client work: native queue installation (#35).

## Development

Bun drives the frontend toolchain; Rust and Bun are the only requirements (plus the Tauri
prerequisites for the desktop target).

```bash
bun install          # frontend dependencies
bun run typecheck    # TypeScript
bun run test         # vitest
bun run tauri dev    # desktop app with hot reload
bun run tauri build  # Windows installer (NSIS + MSI)
```

Core Rust checks run from `src-tauri` without the desktop runtime feature, so the fake-adapter IPPS
integration tests do not require GTK or a native window system:

```bash
cargo fmt --all -- --check
cargo clippy --no-default-features --all-targets -- -D warnings
cargo test --no-default-features
```

The default `desktop` feature enables Tauri/Wry for the Windows application and needs that target's
Tauri prerequisites, so the window, tray, and IPC glue in `lib.rs` and `src/ipc` are compiled and
checked by the Windows CI job (`.github/workflows/desktop-ci.yml`) rather than on a Linux
workstation.

`tests/sharing.rs` covers query behavior and sharing lifecycle. `tests/print_jobs.rs` submits a
Print-Job through a live TLS/IPPS endpoint with a fake printer adapter, including authorization,
queue selection, supported document format, common settings, Stop behavior, and the failure the
server user sees when the spooler refuses a job.
`tests/server_connections.rs` verifies first-use review without a printer query, persistent approval,
changed-certificate blocking, and explicit reapproval.
`tests/client_proxy.rs` sends a native-queue IPP request through the loopback proxy, the pinned TLS/IPPS
endpoint, and a fake printer adapter; it checks common settings, that a local driver cannot
override the configured Network Channel, and the failure the client user sees when forwarding fails
or the server rejects the job. A Windows machine with an installed native IPP queue is
still needed to smoke-check the spooler-to-loopback path.
`tests/legacy_import.rs` runs the import from the previous .NET app against real files: what the
report says, that the previous files are untouched, that queues are never activated, and that
repeating the import changes nothing.
`tests/discovery.rs` runs the real advertiser and the real browser over real sockets: a server appears
with the queues it shares, disappears when sharing stops, a discovered server still requires
fingerprint approval before any printer is listed, and a manual address still works when discovery
reaches nothing. Two servers that advertise the same label stay two entries, because a nearby server
is identified by the address a user reviews. The tests ask loopback instead of the multicast group,
because a test machine usually cannot take port 5353 from whatever multicast DNS responder it already
runs; the wire format, the query/answer exchange, the withdrawal, and the cache are the production
ones.

On Windows, the real-queue spooler smoke test is ignored by default. From `apps/desktop/src-tauri`,
set a local PCL-capable queue and run:

```powershell
$env:SHAPRINT_WINDOWS_SMOKE_PRINTER = "Your local printer queue"
cargo test --no-default-features --lib smoke_submits_a_print_ready_page_to_a_real_queue -- --ignored --nocapture
```

Confirm the test passes and the printer produces the PCL smoke page. This covers the Windows spooler
adapter; native app and physical-printer output still require a Windows machine with an installed
PCL-capable printer.

## Layout

```
src/                 React UI: typed IPC clients, status/sharing/trust hooks, settings and printer panels
src-tauri/src/
  domain/            Printer queues, the server fingerprint, nearby servers, print failures, codes
  application/       Runtime coordinator, sharing, discovery, setup, startup, migration use cases
  adapters/          Print spooler, IPPS endpoint, client TLS trust, identity, discovery, elevation,
                     login registration, DPAPI, previous-app settings
  ipc/               Tauri command adapters, serializable DTOs, status event bridge
```

Boundaries follow ADR 0002; ADR 0003 records how the IPPS endpoint, its identity, and the elevated
setup action work, ADR 0005 the login/tray lifecycle, ADR 0006 the migration from the .NET app, and
ADR 0007 how print failures reach the window. Command handlers only validate, translate, and
delegate; domain rules stay independent of Tauri and the operating system; secrets and print job
content never reach logs, status payloads, or the UI.

## Finding nearby servers

While sharing runs, the server answers queries for `_shaprint-ipps._tcp.local.` and carries one
`queue` entry per shared printer, so a client can show what a server shares before it trusts it.
Stopping sharing withdraws the advertisement: the server sends the withdrawal back to the clients
that asked recently, and answers no further query. A client that sees no withdrawal still forgets an
advertisement within seconds, so a server that crashes does not linger in the list. The responder
takes multicast DNS port 5353 when it is free and 5354 otherwise; a client asks on both, because the
operating system's own multicast DNS responder usually holds 5353.

A browsing client needs nothing from the firewall: it asks from its own port and only hears the
answer. A server has to accept the query, so the sharing panel's single administrator action allows
inbound UDP on the discovery ports as well as inbound TCP on the endpoint's port. Discovery does not
cross subnets, so the address field stays the path for a server on another network.

## Sharing over IPPS

While server sharing runs, the endpoint listens on port 8631, answers IPP printer queries, and
accepts `Print-Job` only when the request carries the configured Network Channel and targets a queue
currently shared. It advertises and accepts only `application/octet-stream` printer-ready spool data;
other document formats receive IPP `client-error-document-format-not-supported` without submission.
The port is not 631: that belongs to Windows' own IPP service. Clients verify and explicitly approve
the server certificate before querying printers; changed fingerprints block queries until explicit
reapproval. Because the first connection from another computer has to pass the Windows firewall,
the sharing panel offers the one action that asks for administrator permission.
