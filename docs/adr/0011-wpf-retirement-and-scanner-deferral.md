# Retire .NET WPF application and defer scanner sharing to Phase 2

---
status: accepted
---

ShaPrint v2.0.0-community marked the maintenance freeze of the original .NET 8 WPF application
(`ShaPrint.WpfApp`, `ShaPrint.Core`, `ShaPrint.Updater`, and `ShaPrint.Tests`). The product path is
now the modern Tauri desktop application (`apps/desktop`), which communicates over standard
IPPS (ADR 0001, ADR 0003) and mDNS discovery (ADR 0004), rendering client raster pages through the
server printer driver (ADR 0010).

To eliminate technical debt, multi-stack maintenance overhead, and dual-pipeline friction, the
legacy C# codebase and Inno Setup installer are fully retired from the active development tree and
archived to the permanent `legacy/wpf-lts` branch and `v2.0.0-community` git tag.

## Non-print features and scanner deferral

The legacy .NET application included remote scanner sharing via Windows Image Acquisition (WIA 2.0).
In the Tauri IPP migration (ADR 0001, ADR 0007), scanner sharing and background auto-updating are
deliberately left behind for the print-sharing MVP.

Scanner sharing is deferred to a future Phase 2. The core domain model (`CONTEXT.md`) remains strictly
focused on print sharing (*Server*, *Client*, *Shared printer*, *Print job*, *Network Channel*,
*Nearby server*, *Client queue*). When scanner support is revisited in a future phase, it will be
designed under its own protocol boundary rather than coupling to IPP printing.

## Consequences

- The repository root becomes a clean Bun workspace managing `apps/desktop`.
- All legacy C# projects, `.sln`, `NuGet.Config`, and `installer.iss` are removed from `dev-tauri` and future `main`.
- LTS fixes for the v2.x WPF application must be branched from `legacy/wpf-lts`.
- The first stable release of the new desktop application is versioned as `v3.0.0`, reflecting breaking architectural changes and standard IPPS protocol replacement.
- CI/CD release workflows now target Windows Tauri packaging (`release-desktop.yml`), producing native NSIS and MSI installers.
- Migration of existing users relies on `legacy_import` (ADR 0007) to carry over configured Network Channels without keeping legacy runtime components.
