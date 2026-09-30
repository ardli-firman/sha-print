# ShaPrint desktop app

Tauri shell for the Windows-only IPP print-sharing MVP (issues #29–#33). It replaces the WPF
application as the product path and lives beside the .NET projects while the transition lasts.

## What exists today

- A Tauri window rendered by React + TypeScript, with one modular Rust crate in `src-tauri`.
- A runtime coordinator that owns the client proxy and server sharing runtimes: it starts them,
  publishes their live status to the window, and stops them cleanly when the window closes.
- Server sharing: the user selects local Windows printer queues and starts or stops sharing
  explicitly. The IPPS endpoint answers `Get-Printers` and `Get-Printer-Attributes`, and accepts
  `Print-Job` only for a selected shared queue when the configured Network Channel matches.
- Windows spooler submission preserves common media size, color, duplex, and copy settings. A fake
  adapter exercises the real TLS/IPPS request path without printing during integration tests.
- The server identity's SHA-256 fingerprint stays stable in app data. A client can manually enter a
  host or host:port, inspect the presented fingerprint without querying printers, explicitly approve
  it, and list printers only while the live fingerprint matches the saved approval. Changed
  fingerprints remain blocked until explicit reapproval.
- Network Channel storage retains only a SHA-256 verifier; the value is never returned through IPC
  or written to logs, status, or UI after configuration.
- One elevated setup action: letting clients reach the endpoint through the Windows firewall. Every
  other action — selecting queues, start/stop, fingerprint review, and channel configuration — runs
  unprivileged.
- Typed IPC: commands and status events use serializable DTOs, and failures carry stable codes
  (`invalid-input`, `unknown-service`, `invalid-state`, `timeout`, `unsupported`, `internal`).
- Least-privilege capabilities: the main window may call the shell's commands and listen for status
  events, and nothing else (no shell, filesystem, dialog, or remote content access).

Remaining client work: the local authenticated print proxy (#34), native queue installation (#35),
and automatic server discovery (#36).

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
Tauri prerequisites.

`tests/sharing.rs` covers query behavior and sharing lifecycle. `tests/print_jobs.rs` submits a
Print-Job through a live TLS/IPPS endpoint with a fake printer adapter, including authorization,
queue selection, supported document format, common settings, and Stop behavior.
`tests/server_connections.rs` verifies first-use review without a printer query, persistent approval,
changed-certificate blocking, and explicit reapproval.

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
  domain/            Printer queues, the server fingerprint, setup policies, stable error codes
  application/       Runtime coordinator, sharing, setup use cases
  adapters/          Print spooler, IPPS endpoint, client TLS trust, identity, and elevation
  ipc/               Tauri command adapters, serializable DTOs, status event bridge
```

Boundaries follow ADR 0002, and ADR 0003 records how the IPPS endpoint, its identity, and the
elevated setup action work. Command handlers only validate, translate, and delegate; domain rules
stay independent of Tauri and the operating system; secrets and print job content never reach logs,
status payloads, or the UI.

## Sharing over IPPS

While server sharing runs, the endpoint listens on port 8631, answers IPP printer queries, and
accepts `Print-Job` only when the request carries the configured Network Channel and targets a queue
currently shared. It advertises and accepts only `application/octet-stream` printer-ready spool data;
other document formats receive IPP `client-error-document-format-not-supported` without submission.
The port is not 631: that belongs to Windows' own IPP service. Clients verify and explicitly approve
the server certificate before querying printers; changed fingerprints block queries until explicit
reapproval. Because the first connection from another computer has to pass the Windows firewall,
the sharing panel offers the one action that asks for administrator permission.
