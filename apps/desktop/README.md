# ShaPrint desktop app

Tauri shell for the Windows-only IPP print-sharing MVP (issues #29 and #30). It replaces the WPF
application as the product path and lives beside the .NET projects while the transition lasts.

## What exists today

- A Tauri window rendered by React + TypeScript, with one modular Rust crate in `src-tauri`.
- A runtime coordinator that owns the client proxy and server sharing runtimes: it starts them,
  publishes their live status to the window, and stops them cleanly when the window closes.
- Typed IPC: commands and status events use serializable DTOs, and failures carry stable codes
  (`invalid-input`, `unknown-service`, `invalid-state`, `timeout`, `internal`).
- Least-privilege capabilities: the main window may call the shell's commands and listen for
  status events, and nothing else (no shell, filesystem, dialog, or remote content access).

The protocol work inside those runtimes arrives with its own issues: IPPS sharing (#31),
authorized job submission (#32), discovery and trust (#33/#36), and the client proxy (#34/#35).

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

## Layout

```
src/                 React UI: typed IPC client, status hook, status panel
src-tauri/src/
  domain/            Service identity, lifecycle rules, stable error codes (no OS/Tauri deps)
  application/       Runtime coordinator and the `RuntimeService` seam it supervises
  adapters/          Service implementations; Windows APIs stay behind this boundary
  ipc/               Tauri command adapters, serializable DTOs, status event bridge
```

Boundaries follow ADR 0002: command handlers only validate, translate, and delegate; domain rules
stay independent of Tauri and the operating system; secrets and print job content never reach
logs, status payloads, or the UI.
