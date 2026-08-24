# Gate 1: Build Foundation Evidence

## Result

PASS

## Verified Commit

The verified code is `a1702774d969684f6389e97718b7817e74e4f8a0` (`ci: add deterministic release verification`), the parent of the commit that adds this evidence file.

## Commands

- Original IPP regression filter (2026-08-24): `dotnet test ShaPrint.Tests/ShaPrint.Tests.csproj -c Release --no-build --disable-build-servers -m:1 -p:UseSharedCompilation=false --filter "FullyQualifiedName~IppServerTests.PrintJob_SendsDataToSpooler_ReturnsJobId|FullyQualifiedName~IppServerTests.PrintJob_LargeDocument_Works|FullyQualifiedName~IppServerTests.InvalidIppVersion_ReturnsError|FullyQualifiedName~IppServerTests.ConcurrentRequests_DoNotCrash" --logger "console;verbosity=minimal"` — 4/4 passed, 0 failed, 0 skipped.
- Concurrency stress loop (2026-08-24): 20 sequential executions of the JobManager, IPP request, and printer-route concurrency regressions — every run passed 3/3; 60 test executions total, 0 failed.
- Canonical verification (2026-08-24): `pwsh -NoProfile -File scripts/verify-build.ps1` — restore exited 0, Release build exited 0, test exited 0; 392/392 passed, 0 failed, 0 skipped. TRX: `TestResults/ShaPrint.Tests.trx` (overwritten by fresh local verification at 09:50:40 local time).
- Workflow wiring: both beta and stable workflows include `scripts/**`, `Verify Release Build`, `./scripts/verify-build.ps1`, and `Upload Test Results`.
- Residue and diff checks: no `[DEBUG-` matches in the Gate 1 IPP sources, IPP tests, or verification script; `git diff --check` exited 0; worktree was clean before this evidence file was added. The Gate 1 diff contains only the documented IPP fixes/tests, release verification workflow/script, and associated planning/state-ignore files.

## Build Warnings

- The fresh canonical Release build succeeded with existing warnings in legacy/deprecated driver paths and tests, including obsolete driver/virtual-printer/named-pipe usage, `DriverPackageService.cs` nullable dereference warning, and existing test warnings (`CS0219`, `CS1998`). Gate 1 introduced no warning suppression and did not add warning-as-error requirements.

## Gate 2 Inputs

- Version and acknowledge the legacy TCP protocol.
- Add bounded network and spooler timeouts.
- Normalize actionable failure categories.
- Test cleanup and overload behavior.
