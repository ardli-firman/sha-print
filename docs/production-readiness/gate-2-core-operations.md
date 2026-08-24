# Gate 2: Core Operations Evidence

## Result

PASS for the LAN deployment profile: one Windows server, approximately three clients, IPP/driverless printing as the primary path, and legacy transport retained for compatibility.

## Verified Commit

The verified implementation is `023df35` (`fix(ipp): advertise client-reachable printer endpoints`). The canonical verification below was run from this commit before this evidence update.

## Canonical Verification

- `pwsh -NoProfile -File scripts/verify-build.ps1` — restore exited 0, Release build exited 0, and the Release test run exited 0.
- Build result: 0 errors. The final incremental build reported 0 warnings.
- Test result: **484/484 passed, 0 failed, 0 skipped** in 12 seconds.
- TRX: `TestResults/ShaPrint.Tests.trx`.
- `git diff --check` exited 0; no `[DEBUG-` markers remain in production or test C# sources; no generated `*_wpftmp.csproj` residue remained after verification.

## Gate 2 Focused Evidence

- IPP protocol and routing: **46/46 passed**. Coverage includes encoded printer-name routing, client-reachable `printer-uri-supported` metadata, full `/printers/{name}/ipp/print` job routing, and concurrent multi-printer handling.
- Driver package transfer and safety: **48/48 passed**. Coverage includes strict 64-hex package IDs, bounded packet framing, cancellation/timeout classification, chunk ordering and size limits, SHA-256 verification, atomic publication, active-cache eviction protection, stalled-transfer cleanup, child-process-tree termination, and same-driver concurrent export serialization.
- Discovery and monitor networking: **90/90 passed**. Coverage includes authenticated framing, bounded total and idle deadlines, deduplication, request/work limits, tracked start/stop lifecycle, cancellation propagation, bounded worker shutdown, restart/stop race handling, and actionable failure categories.
- Scanner and payload validation: **11/11 scanner + 5/5 payload passed**.
- Earlier Gate 2 codec/payload and server operation slices remain green: **27/27**, **43/43**, and **14/14** focused passes as recorded by their reviews.

All Gate 2 implementation slices were reviewed by a separate subagent. Final approvals are recorded for Task 4 (scanner), Task 5 (driver transfer), and Task 6 (discovery/monitor).

## Build Warnings

The repository still contains pre-existing warnings in deprecated compatibility paths and tests (for example obsolete `DriverPackageManager`, `DriverInstaller`, `PipeListener`, and `VirtualPrinterManager` usages, plus existing `CS0219`/`CS1998` test warnings when those files are rebuilt). These paths are intentionally retained for legacy-client compatibility; no warning was promoted to an error or suppressed by Gate 2.

## Hardware-Only Validation

The automated environment cannot exercise a physical Windows Print Spooler queue, a real IPP printer, or a WIA scanner. Before LAN rollout, validate on the target server and one client:

1. Add the printer through its `ipp://` endpoint using Windows driverless discovery.
2. Submit a small and a multi-page job from each of the three clients.
3. Confirm server restart/login auto-start, monitor discovery, and clean shutdown.
4. Exercise one WIA scan and one legacy-client print if those compatibility paths are required.

These are deployment acceptance checks, not unresolved automated-test failures.

## Gate 3 Inputs

- Run the Windows hardware acceptance checklist above.
- Validate packaging, updater rollback, startup registration, firewall/port rules, and operator-facing recovery instructions on the target LAN.
- Preserve the default LAN network behavior and existing authentication contract selected for this deployment.
