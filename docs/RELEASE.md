# ShaPrint Release Strategy

> **Status:** Active development has transitioned to **ShaPrint v3.0.0** (Tauri + Rust + React).
> The legacy C# WPF application is maintained on the `legacy/wpf-lts` branch at v2.0.0-community.

## v3.0.0 (Modern Tauri Desktop)

The current product architecture:
- Windows desktop app built with **Tauri 2**, **React 19**, and a modular **Rust** core (`apps/desktop`).
- Standards-based printer sharing using **IPPS** (Internet Printing Protocol over TLS) and **mDNS discovery**.
- Client PWG Raster rendering directly through server manufacturer print drivers (ADR 0010).
- Windows elevated setup helper for firewall configuration and native queue creation (`Add-Printer -IppURL`).
- Zero plaintext credential leakage: Network Channel verifiers use salted SHA-256 and Windows DPAPI protection.
- Distributed as native NSIS and MSI installers built via GitHub Actions (`release-desktop.yml`).

## v2.0.0-community (LTS Archive)

The legacy .NET 8 WPF application:
- Archived to the permanent Git branch `legacy/wpf-lts` and tagged `v2.0.0-community`.
- Licensed under **GPL v3**.
- Contains proprietary TCP print sharing, UDP discovery, and WIA scanner sharing.
- Critical security or maintenance fixes for the WPF codebase should be branched from `legacy/wpf-lts`.
