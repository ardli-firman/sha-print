# ShaPrint Production Readiness Roadmap

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement each gate plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Qualify the complete ShaPrint application for production use on a trusted internal LAN.

**Architecture:** Delivery is split into six independently reviewable gates. Each gate produces working, testable software and evidence that becomes input to the next gate; detailed plans are written just-in-time so later plans use verified interfaces rather than guesses.

**Tech Stack:** .NET 8, C#, WPF, WinForms updater, SharpIppNext, xUnit, PowerShell, Inno Setup, GitHub Actions, Windows 10/11.

## Global Constraints

- Target one server and approximately three clients per room.
- Support Windows 10 and Windows 11 x64.
- Start automatically after the Windows user logs in; pre-login service operation is out of scope.
- Fail network operations immediately with actionable messages; do not automatically retry jobs.
- Remove document payloads after success, failure, or cancellation; persist sanitized metadata only.
- Keep auto-update at startup and restore the previous version when the new version is unhealthy.
- Store logs and crash reports locally.
- Permit `DefaultChannel` and retain current Network Channel security without additional IPP authentication.
- Keep IPP/driverless, legacy printing, scanner sharing, driver sharing, discovery, and monitor as supported production features.
- Do not require code signing until a certificate is available.

---

## Gate Plan Sequence

1. **Build Foundation** — `docs/superpowers/plans/2026-08-24-gate-1-build-foundation.md`
   - Repair the current IPP failures.
   - Make IPP job state and HTTP routing concurrency-safe.
   - Add a deterministic local/CI verification entry point.

2. **Core Operations** — write after Gate 1 evidence is green.
   - Version the legacy TCP protocol and add encrypted acknowledgements.
   - Bound print, scan, driver-transfer, discovery, monitor, and spooler timeouts.
   - Normalize actionable error categories across IPP and legacy paths.
   - Verify cleanup and overload behavior.

3. **Application Lifecycle** — write after Gate 2 interfaces stabilize.
   - Give critical services owned task lifecycles and health state.
   - Make startup-after-login and last-mode restoration deterministic.
   - Make shutdown and partial-start failure observable.

4. **Local Operations** — write after service health contracts stabilize.
   - Add persistent structured logging, rotation, sanitization, crash capture, and diagnostics export.

5. **Safe Delivery** — write after health and diagnostics interfaces exist.
   - Make installer operations idempotent.
   - Add verified update staging, graceful shutdown, post-update health check, and automatic rollback.
   - Preserve configuration and clean owned OS resources on uninstall.

6. **Release Qualification** — write after install/update contracts stabilize.
   - Add Windows 10/11 smoke-test matrix and installer/update/rollback automation.
   - Produce release evidence and a manual one-server/three-client hardware checklist.
   - Promote only qualified beta artifacts to stable.

## Gate Rule

A gate is complete only when its focused tests, the full solution verification command, diff review, and documented evidence pass. A later gate must not compensate for a known failure in an earlier gate.
