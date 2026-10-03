# Tauri app structure and Rust boundaries

---
status: accepted
---

The new desktop app will live in `apps/desktop/` beside the existing .NET projects while migration continues. It will use React and TypeScript with one modular Rust crate under Tauri's `src-tauri` directory, organized around thin IPC commands, application use cases, domain types, and adapters for IPP, discovery, storage, and operating-system APIs. Windows integrations will stay behind platform adapters from the start so the later Linux phase can add its own implementations without moving domain behavior. A runtime coordinator will own background services, and setup actions that need administrator rights will run through a narrow elevated helper while the main app stays unprivileged.

## Consequences

- Cargo does not enforce boundaries inside the single crate, so modules must keep their public interfaces narrow.
- Tauri capabilities will grant each window only the permissions it needs.
- Rust recoverable errors will use typed results with stable codes at the IPC boundary; logs will omit secrets and print-job content.
- Async network and disk work will not block the runtime. Synchronous Windows APIs will run behind blocking adapters.
