<div align="center">
  <h1>ShaPrint</h1>
  <p><b>Modern, Standards-Based LAN & Cross-VLAN Printer Sharing for Windows</b></p>
  <p><i>Built with Tauri 2, React, and Rust</i></p>
</div>

---

**ShaPrint** is a modern desktop printer-sharing solution for Windows. It allows computers on a local network or across VLANs to share physical printers securely and transparently without relying on problematic Windows SMB printer sharing or shared domain accounts.

Communication runs entirely over standard **IPPS (Internet Printing Protocol over TLS)** on port `8631`, local client queue routing via proxy on port `8632`, and zero-configuration **multicast DNS discovery** (ADR 0001–0010).

---

## ✨ Key Capabilities

- 🖨️ **Standards-Based IPPS:** Server shares queues using standard IPP over TLS with pinned SHA-256 certificate fingerprints.
- 🎨 **Driver Fidelity:** Renders client PWG Raster pages directly through the server's installed manufacturer driver (e.g. Epson, Canon, HP), preserving media size, color, duplex, and copies (ADR 0010).
- 🔍 **Zero-Config Discovery:** Nearby servers advertise shared queues over multicast DNS (`_shaprint-ipps._tcp.local.`), while manual IP entry connects across subnets and VLANs (ADR 0004).
- 🔒 **Defense-in-Depth Security:** Network Channel passwords are never sent in plaintext or stored raw; servers authenticate clients with salted SHA-256 verifiers and DPAPI encryption.
- 🪟 **Native Windows Queues:** Installs native Windows print queues via `Add-Printer -IppURL` routing through a transparent local loopback proxy (ADR 0005).
- 🔄 **Legacy Transition:** Seamlessly imports configured Network Channels from previous .NET ShaPrint installations read-only without disturbing existing setups (ADR 0007).
- 👻 **Tray & Lifecycle:** Starts silently on user login (per-user Run key, no elevation required), minimizes to the system notification area, and manages sharing runtimes cleanly (ADR 0006).

---

## 🏗 Architecture & Documentation

All architectural decisions are documented under [`docs/adr/`](docs/adr/):
- [ADR 0001: IPP print sharing protocol](docs/adr/0001-ipp-print-sharing.md)
- [ADR 0002: Tauri app structure and Rust boundaries](docs/adr/0002-tauri-app-structure.md)
- [ADR 0003: IPPS sharing endpoint and identity](docs/adr/0003-ipps-sharing-endpoint.md)
- [ADR 0004: Server discovery over multicast DNS](docs/adr/0004-server-discovery.md)
- [ADR 0005: Native client queue installation](docs/adr/0005-native-client-queue-installation.md)
- [ADR 0006: Desktop lifecycle and login startup](docs/adr/0006-desktop-lifecycle.md)
- [ADR 0007: Migrating settings from legacy .NET app](docs/adr/0007-settings-migration.md)
- [ADR 0008: Print failure reporting](docs/adr/0008-print-failure-reporting.md)
- [ADR 0010: Render client raster through server driver](docs/adr/0010-render-client-raster-through-server-driver.md)
- [ADR 0011: Retire .NET WPF application & defer scanner sharing](docs/adr/0011-wpf-retirement-and-scanner-deferral.md)

---

## 💻 Development Setup

Requirements:
- [Bun](https://bun.sh) (v1.0+)
- [Rust](https://rustup.rs) (stable)
- Windows 10/11 with C++ Build Tools (for Tauri desktop target)

### Getting Started

```bash
# Install frontend dependencies
bun install

# Run frontend development with hot-reload
bun run dev

# Run full desktop app in development
bun run tauri dev

# Typecheck and run tests
bun run typecheck
bun run test

# Rust checks (inside apps/desktop/src-tauri)
cd apps/desktop/src-tauri
cargo test --no-default-features
cargo clippy --no-default-features --all-targets -- -D warnings
```

---

## 📦 Building Releases

```bash
# Build native Windows NSIS installer and MSI bundle
bun run tauri build
```

Built artifacts are output to `apps/desktop/src-tauri/target/release/bundle/`.

---

## 📜 Legacy Version

The legacy .NET 8 WPF application has been frozen at `v2.0.0-community` and archived to the permanent [`legacy/wpf-lts`](../../tree/legacy/wpf-lts) branch.

---

<div align="center">
  <b>ShaPrint Open Source Project</b>
</div>
