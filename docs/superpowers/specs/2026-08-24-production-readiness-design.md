# ShaPrint Production Readiness Design

**Date:** 2026-08-24

**Status:** Approved

**Target:** Internal LAN deployment, one server and approximately three clients per room

## Objective

Make the entire ShaPrint application production-ready for internal LAN use on Windows 10 and Windows 11. IPP/driverless printing, legacy printing, scanner sharing, driver sharing, discovery, monitoring, startup, installer, updater, and rollback all remain supported production features.

Production-ready means every automated gate passes and the hardware qualification checklist is completed. Capabilities that cannot be exercised in CI must be recorded as manual validation rather than assumed to pass.

## Operating Constraints

- One room has one server and approximately three clients.
- The application starts automatically after the Windows user logs in; operation before login is not required.
- Network failures fail immediately with an actionable message. ShaPrint does not automatically retry jobs.
- Document payloads are removed immediately after success, failure, or cancellation. Only sanitized metadata may be retained.
- Auto-update runs at startup and automatically restores the previous version if the new version is unhealthy.
- Logs and crash reports remain local for now.
- `DefaultChannel` remains permitted. Existing Network Channel security is retained without additional IPP authentication.
- IPP/driverless is mandatory and remains available to trusted devices on the internal LAN.
- A Windows code-signing certificate is not currently available.

## Delivery Strategy

Use staged production gates. Each gate must pass before work proceeds to the next gate. Changes are small, test-first vertical slices; unrelated refactoring is excluded.

### Gate 1: Build Foundation

- Restore a deterministic green build and test baseline.
- Correct the current IPP request fixtures, invalid-version response behavior, and concurrency failures.
- Make CI output and dependency restore deterministic.

### Gate 2: Core Operations

- Stabilize IPP and legacy print, scan, discovery, monitor, driver sharing, and spooler integration.
- Add bounded timeouts, cancellation, thread-safe shared state, overload rejection, and actionable error responses.
- Keep IPP and legacy paths as first-class supported paths.

### Gate 3: Application Lifecycle

- Give every background/network service explicit start, stop, cancellation, and health state.
- Eliminate unobserved fire-and-forget work for critical services.
- Make last-mode startup after login deterministic.
- Handle port conflicts and partial startup as visible degraded states.
- Close listeners, streams, tasks, and synchronization primitives cleanly on shutdown.

### Gate 4: Local Operations

- Add persistent structured logs under `%LocalAppData%\ShaPrint\Logs`.
- Include timestamp, severity, component, and correlation/job ID.
- Rotate logs by age and size.
- Exclude document contents, Network Channel values, and sensitive material.
- Add local diagnostic export containing logs, versions, service health, mode, and sanitized configuration.

### Gate 5: Safe Delivery

- Make installation, upgrade, downgrade, uninstall, and rollback deterministic and logged.
- Preserve user configuration across upgrades.
- Remove owned scheduled tasks and firewall rules during uninstall.
- Verify release artifacts before installation and health-check the new version before committing an update.

### Gate 6: Release Qualification

- Run automated tests and Windows 10/11 smoke tests.
- Perform the physical hardware checklist.
- Promote only qualified beta artifacts to stable.
- Retain checksums, test results, installer logs, dependency versions, and known limitations as release evidence.

## Job Lifecycle and Error Handling

Every print or scan operation follows:

`Accepted -> Validated -> Processing -> Completed | Failed`

- IPP returns a standards-valid IPP response.
- The legacy TCP protocol gains a versioned encrypted acknowledgement so the client can distinguish accepted, rejected, and spooler-failed jobs.
- Compatibility is version-based rather than inferred from an ambiguous first integer.
- Connection, payload-read, and spooler operations have bounded timeouts.
- No automatic job retry occurs.
- User-facing failures distinguish at least: server offline, channel mismatch, invalid payload, unavailable target, overload, timeout, and spooler rejection.
- Shared job IDs, connection sets, server caches, and related collections are thread-safe.
- The concurrency limit remains intentionally small for the target room scale; overload is rejected explicitly.

## Payload Lifetime

- Document bytes remain only in memory or in Windows-owned temporary spool storage required to execute the operation.
- ShaPrint does not persist document content for troubleshooting.
- ShaPrint-owned temporary files and references are cleaned on success, failure, cancellation, startup recovery, and rollback.
- Logs contain sanitized metadata only.

## Service Lifecycle

- The application host owns all critical service tasks.
- Each service exposes explicit startup, shutdown, cancellation, and health behavior.
- Critical task failure is observed, logged, and reflected in UI health.
- Fatal unhandled exceptions are logged and terminate or isolate the failed component; they are not silently marked handled.
- Startup at user login restores the selected Server, Client, or Monitor mode.

## Update Transaction and Rollback

1. Download the installer into a unique staging directory.
2. Verify expected size and SHA-256 from trusted release metadata.
3. Back up the active application version and required configuration.
4. Request graceful application shutdown; force termination only after a bounded timeout.
5. Run the installer and check its exit status.
6. Health-check executable startup, installed version, configuration loading, and background-mode initialization.
7. On failure, restore and restart the previous version.
8. On success, remove stale staging and retain only a bounded rollback history.

Without a code-signing certificate, SHA-256 verification and the official GitHub release source are the release integrity controls. Documentation must disclose possible SmartScreen warnings.

## Test Strategy

### Unit

- Protocol parsing and version negotiation
- Job state transitions and failure mapping
- Concurrency and unique identifiers
- Validation, timeout, cancellation, and cleanup
- Log sanitization and rotation
- Version selection and rollback decisions

### Integration

- IPP request/response against a simulated spooler
- Legacy encrypted print acknowledgement
- Encrypted scan and monitor flows
- Discovery and server identity behavior
- Service start/stop and degraded startup
- Installer/updater orchestration using process and filesystem abstractions

### Installer and Update

- Clean install and repeated install
- Upgrade and controlled downgrade
- Failed update and automatic rollback
- Configuration preservation
- Uninstall cleanup
- No orphaned processes, scheduled tasks, firewall rules, staging files, or document payloads

### Platform and Hardware

- Automated build and smoke-test matrix for Windows 10 and Windows 11.
- Manual qualification with one server, three clients, at least one physical printer, and one physical scanner.
- Scenarios include normal print/scan, server offline, network disconnect, spooler/printer error, login startup, shutdown, successful update, failed update, and rollback.

## Release Flow

- `develop` produces a beta candidate artifact after automated gates pass.
- The beta candidate undergoes Windows and hardware qualification.
- Stable promotion uses the exact qualified artifact or a reproducibly identical artifact; stable is not rebuilt from unqualified source state.
- Release evidence includes test results, checksum, installer/update logs, dependency inventory, and known limitations.

## Definition of Done

- All solution tests and Release builds pass deterministically.
- All supported feature paths have appropriate unit/integration coverage.
- Required Windows 10/11 and hardware scenarios pass.
- Failure messages are actionable and no automatic job retry occurs.
- Document payloads are not retained by ShaPrint.
- Critical background work is owned and observable.
- Local logs rotate and diagnostic export is sanitized.
- Installation, update, health check, and rollback are verified.
- The release artifact and evidence are reproducible and retained.

## Explicit Non-Goals

- Operation before Windows user login
- Cloud telemetry or remote crash reporting
- Cross-room scale or high-availability clustering
- Additional authentication for trusted-LAN IPP traffic
- Mandatory replacement of `DefaultChannel`
- Code signing before a certificate becomes available
