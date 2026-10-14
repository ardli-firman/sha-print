<div align="center">
  <h1>ShaPrint</h1>
  <p><b>Modern, Standards-Based LAN & Cross-VLAN Printer Sharing for Windows and Android</b></p>
  <p><i>Desktop (Tauri 2, React, Rust) & Mobile (Android, Kotlin)</i></p>
</div>

---

**ShaPrint** is a modern printer-sharing solution for local area networks (LAN) and cross-VLAN environments. It allows computers and mobile devices to share and access physical printers securely and transparently without relying on problematic Windows SMB printer sharing, third-party cloud services, or shared domain accounts.

Communication runs entirely over standard **IPPS (Internet Printing Protocol over TLS)** on dedicated high port TCP `48631`, local client queue routing via proxy on TCP `48632` (`127.0.0.1:48632`), and zero-configuration **multicast DNS discovery** on UDP `48633` ([ADR 0001–0017](docs/adr/)).

---

## 📥 Downloads & Supported Platforms

| Platform | Role | Package / Format | Download / Instructions |
|---|---|---|---|
| **🪟 Windows (10 / 11)** | Server & Client (Full GUI + Spooler) | `.msi`, `.exe` Installer | [⬇️ Desktop Releases (`v*.*.*`)](https://github.com/ardli-firman/sha-print/releases?q=tag%3Av*&expanded=true) |
| **📱 Android (8.0+)** | Mobile Client (Direct & PrintService) | `.aab` (Play Store) & `.apk` | [⬇️ Android Releases (`android-v*.*.*`)](https://github.com/ardli-firman/sha-print/releases?q=tag%3Aandroid*&expanded=true) |
| **🐧 Ubuntu / Debian** | Headless Server Daemon | `.deb` (Ubuntu 22.04 / 24.04) | [📦 DEB Packaging Guide](packaging/linux/deb) (`make build-deb`) |
| **🐧 AlmaLinux / RHEL** | Headless Server Daemon | `.rpm` (AlmaLinux 9 / 10) | [📦 RPM Packaging Guide](packaging/linux/rpm) (`make build-rpm`) |

---

## 🌐 Dedicated Network Ports

ShaPrint binds fixed, dedicated high ports to eliminate collisions with Windows system responders or common local developer servers ([ADR 0014](docs/adr/0014-unique-ports-and-cross-vlan-reachability.md)):

| Service | Protocol / Transport | Port | Purpose | Reference |
|---|---|---|---|---|
| **Server Sharing Endpoint** | IPPS (TLS) | TCP `48631` | Encrypted IPP server (`/ipp/print/...`) answering printer queries and processing raster print jobs | [ADR 0003](docs/adr/0003-ipps-sharing-endpoint.md), [ADR 0014](docs/adr/0014-unique-ports-and-cross-vlan-reachability.md) |
| **Client Loopback Proxy** | IPP (HTTP loopback) | TCP `48632` | Local Windows spooler routing (`127.0.0.1:48632`) attaching Network Channel credentials | [ADR 0005](docs/adr/0005-native-client-queue-installation.md), [ADR 0014](docs/adr/0014-unique-ports-and-cross-vlan-reachability.md) |
| **Discovery Responder / Querier** | mDNS (UDP multicast) | UDP `48633` | Advertising and discovering nearby servers (`_shaprint-ipps._tcp.local.`, TTL = 32) | [ADR 0004](docs/adr/0004-server-discovery.md), [ADR 0014](docs/adr/0014-unique-ports-and-cross-vlan-reachability.md) |

*If a port is held by a stale `shaprint` process on startup, ShaPrint automatically reclaims it; third-party conflicts are reported cleanly without killing foreign processes.*

---

## ✨ Key Capabilities

- 🖨️ **Standards-Based IPPS:** Server shares queues using standard RFC 8010/8011 IPP over TLS with pinned SHA-256 certificate fingerprints (Trust-On-First-Use) and a dedicated TCP `48631` listener ([ADR 0001](docs/adr/0001-ipp-print-sharing.md), [ADR 0003](docs/adr/0003-ipps-sharing-endpoint.md), [ADR 0014](docs/adr/0014-unique-ports-and-cross-vlan-reachability.md)).
- 🎨 **Driver Fidelity & Fixed Media Catalog:** Renders client PWG Raster pages directly through the server's installed manufacturer driver (e.g., Epson, Canon, HP), preserving media size, color, duplex, and copies ([ADR 0010](docs/adr/0010-render-client-raster-through-server-driver.md)). Publishes a fixed media catalog with 0.5 mm geometry matching to prevent driver capability mismatches ([ADR 0017](docs/adr/0017-ipp-media-catalog-and-client-queue-repair.md)).
- 🔍 **Zero-Config Discovery & Cross-VLAN Reachability:** Nearby servers advertise shared queues over multicast DNS (`_shaprint-ipps._tcp.local.`, UDP `48633`) across all active non-loopback IPv4 interfaces with TTL = 32 for mDNS reflection ([ADR 0004](docs/adr/0004-server-discovery.md), [ADR 0014](docs/adr/0014-unique-ports-and-cross-vlan-reachability.md)). Manual host:port entry, persistent trusted server storage, and live unicast polling enable seamless cross-VLAN printing.
- 📱 **Native Android Mobile Client:** Lightweight client application in `apps/android` (`io.shaprint.client`) connecting directly over IPPS (TCP `48631`) with client-side 300 dpi PWG raster rendering via `PdfRenderer`, battery-friendly on-demand discovery (UDP `48633`), hardware-backed Android Keystore credential encryption, system `PrintService` integration, and Share Sheet target ([ADR 0016](docs/adr/0016-android-mobile-client.md)).
- 🔒 **Defense-in-Depth Security:** Network Channel passwords are never sent in plaintext or stored raw; servers authenticate clients with salted SHA-256 verifiers, and credentials are encrypted using Windows DPAPI or Android Keystore ([ADR 0003](docs/adr/0003-ipps-sharing-endpoint.md), [ADR 0007](docs/adr/0007-settings-migration.md), [ADR 0016](docs/adr/0016-android-mobile-client.md)).
- 🪟 **Native Windows Queues & One-Click Repair:** Installs native Windows print queues via `Add-Printer -IppURL` routing through a transparent local loopback proxy ([ADR 0005](docs/adr/0005-native-client-queue-installation.md)). Features a dedicated "Repair paper sizes" action to refresh cached driver capabilities cleanly without manual deletion ([ADR 0017](docs/adr/0017-ipp-media-catalog-and-client-queue-repair.md)).
- 🛡️ **Automated Inbound Firewall Provisioning:** Verifies effective Windows Firewall inbound policy on Server Sharing start (TCP `48631`, UDP `48633`) and prompts for UAC elevation only when rules are missing or restricted ([ADR 0013](docs/adr/0013-inbound-access-on-server-start.md)).
- ⚡ **Intent-Driven UI & Unified Add Printer Wizard:** Background services automatically start and stop based on user intent. Includes a streamlined 4-step `AddPrinterDialog` wizard with visual SHA-256 fingerprint chunking (`XXXX • YYYY • ZZZZ • AAAA`), system status pills, and contextual diagnostics ([ADR 0009](docs/adr/0009-desktop-ui-ux-flow-redesign.md)).
- 🔄 **Persistent Server Sharing Across Reboots:** Server shared printer selections and sharing state persist across system reboots (`sharing_state.json`), ensuring workstations acting as print servers resume sharing after Windows updates or power cycles ([ADR 0012](docs/adr/0012-desktop-auto-updater-and-persistent-sharing.md), superseding [ADR 0006](docs/adr/0006-desktop-lifecycle.md)).
- 📦 **Desktop Auto-Updater & Drain Guard:** Unprivileged per-user background updates via `tauri-plugin-updater` with active-job drain protection (8s cooldown), idle tray auto-restart (15-min idle), and mDNS version advertisement (`v=` TXT record) ([ADR 0012](docs/adr/0012-desktop-auto-updater-and-persistent-sharing.md)).
- 🚀 **Manual Nightly Releases:** Maintainer-dispatched nightly builds published as public GitHub prereleases with dedicated update feeds and SemVer prerelease versioning ([ADR 0015](docs/adr/0015-manual-nightly-desktop-releases.md)).
- ⚠️ **Classified Print Failure Reporting:** Surfaces actionable error cards with stable error codes and single-click recovery steps, while strictly omitting secrets and document content from logs and UI ([ADR 0008](docs/adr/0008-print-failure-reporting.md)).
- 👻 **Tray & Lifecycle:** Starts silently on Windows login (per-user `Run` key, no elevation required) and minimizes cleanly to the system notification area ([ADR 0006](docs/adr/0006-desktop-lifecycle.md)).
- 🔄 **Legacy Transition:** Seamlessly imports configured Network Channels from previous .NET ShaPrint installations read-only without disturbing existing setups ([ADR 0007](docs/adr/0007-settings-migration.md)), while legacy .NET WPF is retired to [`legacy/wpf-lts`](../../tree/legacy/wpf-lts) ([ADR 0011](docs/adr/0011-wpf-retirement-and-scanner-deferral.md)).

---

## 📁 Repository Structure

```
sha-print/
├── apps/
│   ├── desktop/            # Tauri 2 desktop application (React, TypeScript, Rust)
│   │   ├── src/            # Frontend UI (Tailwind CSS, shadcn-ui)
│   │   └── src-tauri/      # Rust backend: IPPS server, proxy, mDNS discovery, Windows adapters
│   └── android/            # Native Android mobile client (Kotlin, Jetpack Compose)
│       └── app/            # Android app module (io.shaprint.client, minSdk 26, targetSdk 35)
├── docs/
│   ├── adr/                # Architecture Decision Records (ADRs 0001–0017)
│   └── agents/             # Agent guidelines and triage conventions
├── scripts/
│   └── nightly/            # Automated nightly release tooling
└── package.json            # Monorepo root Bun workspace
```

---

## 🏗 Architecture & Documentation

All architectural decisions are documented under [`docs/adr/`](docs/adr/):

- [ADR 0001: Windows-first IPP print sharing](docs/adr/0001-ipp-print-sharing.md)
- [ADR 0002: Tauri app structure and Rust boundaries](docs/adr/0002-tauri-app-structure.md)
- [ADR 0003: IPPS sharing endpoint and identity](docs/adr/0003-ipps-sharing-endpoint.md)
- [ADR 0004: Server discovery over multicast DNS](docs/adr/0004-server-discovery.md)
- [ADR 0005: Native client queue installation](docs/adr/0005-native-client-queue-installation.md)
- [ADR 0006: Desktop lifecycle and login startup](docs/adr/0006-desktop-lifecycle.md)
- [ADR 0007: Migrating settings from legacy .NET app](docs/adr/0007-settings-migration.md)
- [ADR 0008: Print failure reporting](docs/adr/0008-print-failure-reporting.md)
- [ADR 0009: Desktop UI/UX flow redesign](docs/adr/0009-desktop-ui-ux-flow-redesign.md)
- [ADR 0010: Render client raster through server driver](docs/adr/0010-render-client-raster-through-server-driver.md)
- [ADR 0011: Retire .NET WPF application & defer scanner sharing](docs/adr/0011-wpf-retirement-and-scanner-deferral.md)
- [ADR 0012: Desktop auto-updater, job-aware restart, and persistent server sharing](docs/adr/0012-desktop-auto-updater-and-persistent-sharing.md)
- [ADR 0013: Ensure inbound Client access when Server Sharing starts](docs/adr/0013-inbound-access-on-server-start.md)
- [ADR 0014: Dedicated ports, stale-process port reclamation, and cross-VLAN server reachability](docs/adr/0014-unique-ports-and-cross-vlan-reachability.md)
- [ADR 0015: Manual nightly desktop releases](docs/adr/0015-manual-nightly-desktop-releases.md)
- [ADR 0016: Android mobile client architecture](docs/adr/0016-android-mobile-client.md)
- [ADR 0017: IPP media catalog and client queue repair](docs/adr/0017-ipp-media-catalog-and-client-queue-repair.md)

---

## 💻 Development Setup

### Desktop Application (`apps/desktop`)

**Prerequisites:**
- [Bun](https://bun.sh) (v1.0+)
- [Rust](https://rustup.rs) (stable)
- Windows 10/11 with C++ Build Tools (for native Windows builds)

```bash
# Install frontend dependencies
bun install

# Run frontend development with hot-reload
bun run dev

# Run full desktop app in development
bun run tauri dev

# Typecheck and run frontend tests
bun run typecheck
bun run test

# Rust backend checks and unit tests (from apps/desktop/src-tauri)
cd apps/desktop/src-tauri
cargo fmt --all -- --check
cargo clippy --no-default-features --all-targets -- -D warnings
cargo test --no-default-features
```

### Android Mobile Client (`apps/android`)

**Prerequisites:**
- JDK 17+
- Android SDK (API 35, `minSdk = 26`)

```bash
cd apps/android

# Run unit tests
./gradlew test

# Build debug APK
./gradlew assembleDebug
```

---

## 📦 Building Releases

```bash
# Desktop: Build native Windows NSIS installer and MSI bundle
bun run tauri build

# Android: Build release APK or AAB
cd apps/android
./gradlew assembleRelease
```

Built desktop artifacts are output to `apps/desktop/src-tauri/target/release/bundle/`.

---

## 🚀 Release Channels

- **Stable Channel:** Released as official GitHub Releases (`vX.Y.Z`) from the `main` branch.
- **Nightly Channel:** Dispatched manually via `make release-nightly` or GitHub Actions workflow dispatch. Publishes public GitHub prereleases with unique SemVer prerelease tags (e.g. `v3.4.0-nightly.8`) and an isolated update feed ([ADR 0015](docs/adr/0015-manual-nightly-desktop-releases.md)).

---

## 📜 Legacy Version

The legacy .NET 8 WPF application has been frozen at `v2.0.0-community` and archived to the permanent [`legacy/wpf-lts`](../../tree/legacy/wpf-lts) branch ([ADR 0011](docs/adr/0011-wpf-retirement-and-scanner-deferral.md)).

---

<div align="center">
  <b>ShaPrint Open Source Project</b>
</div>
