# ShaPrint desktop app

Tauri shell for the Windows-only IPP print-sharing MVP (issues #29, #30, and #31). It replaces the
WPF application as the product path and lives beside the .NET projects while the transition lasts.

## What exists today

- A Tauri window rendered by React + TypeScript, with one modular Rust crate in `src-tauri`.
- A runtime coordinator that owns the client proxy and server sharing runtimes: it starts them,
  publishes their live status to the window, and stops them cleanly when the window closes.
- Server sharing: the user selects local Windows printer queues, starts or stops sharing explicitly,
  and an IPPS client can query the shared printers while sharing runs. `Get-Printers` and
  `Get-Printer-Attributes` answer over TLS; print job submission is #32.
- A server certificate identity whose SHA-256 fingerprint is shown for a client to approve, kept in
  the app data directory so it survives restarts.
- One elevated setup action: letting clients reach the endpoint through the Windows firewall. Every
  other action — selecting queues, start/stop, showing the fingerprint — runs unprivileged.
- Typed IPC: commands and status events use serializable DTOs, and failures carry stable codes
  (`invalid-input`, `unknown-service`, `invalid-state`, `timeout`, `unsupported`, `internal`).
- Least-privilege capabilities: the main window may call the shell's commands and listen for status
  events, and nothing else (no shell, filesystem, dialog, or remote content access).

Client-side work arrives with its own issues: authorized job submission (#32), discovery and trust
(#33/#36), the client proxy (#34), and native queue installation (#35).

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

Rust checks run from `src-tauri`:

```bash
cargo fmt --all -- --check
cargo clippy --all-targets -- -D warnings
cargo test
```

`tests/sharing.rs` is the end-to-end seam: it selects queues through a fake printer adapter, starts
sharing, and queries the running endpoint over TLS with a client that approves the server's
fingerprint. The real Windows paths — spooler enumeration and the elevated firewall rule — compile
only on Windows and need a Windows smoke check.

## Layout

```
src/                 React UI: typed IPC client, status and sharing hooks, status and printer panels
src-tauri/src/
  domain/            Printer queues, the server fingerprint, setup policies, stable error codes
  application/       Runtime coordinator, sharing and setup use cases, the ports they read through
  adapters/          Service implementations: print spooler, IPPS endpoint, identity, elevation
  ipc/               Tauri command adapters, serializable DTOs, status event bridge
```

Boundaries follow ADR 0002, and ADR 0003 records how the IPPS endpoint, its identity, and the
elevated setup action work. Command handlers only validate, translate, and delegate; domain rules
stay independent of Tauri and the operating system; secrets and print job content never reach logs,
status payloads, or the UI.

## Sharing over IPPS

While server sharing runs, the endpoint listens on port 8631 and answers IPP queries from clients
that approved the certificate fingerprint shown in the window. The port is not 631: that belongs to
Windows' own IPP service. Because the first connection from another computer has to pass the Windows
firewall, the sharing panel offers the one action that asks for administrator permission.
