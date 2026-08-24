# Gate 2 Core Operations Implementation Plan

> **For agentic workers:** use the subagent-driven task loop: one fresh implementer per task, focused RED/GREEN verification, read-only review, then ledger update before the next task.

**Goal:** Make legacy print/scan/driver-transfer and discovery/monitor operations bounded, observable, and safe for one Windows LAN server with approximately three clients.

**Gate rule:** every task keeps IPP/driverless and existing Network Channel security intact; no automatic job retry; document bytes are never persisted by ShaPrint; all network and device work has a bounded deadline or explicit cancellation.

## Global constraints

- Preserve compatibility with the existing encrypted payload format while introducing an explicit versioned envelope for new legacy clients.
- `DefaultChannel` remains valid; do not add IPP authentication.
- Keep public feature paths: legacy print, IPP, scan, driver sharing, discovery, and monitor.
- Avoid code-signing changes and warning suppression.
- Tests must use abstractions/fakes; no physical printer/scanner is required in CI.

## Task sequence

### Task 1 — Versioned legacy envelope and encrypted acknowledgement

Files: create core legacy protocol codec/models and tests; update `PrintJobPayload` only through a compatibility seam. Define a fixed magic, protocol version, message type, correlation ID, payload length, and AES-GCM-protected ACK with statuses `Accepted`, `InvalidPayload`, `Overloaded`, `TargetUnavailable`, `SpoolerRejected`, `Timeout`, `Canceled`, and `ServerError`. Unknown versions/types and truncated frames fail closed. Existing v2 payload readers remain supported for compatibility.

Verification: codec RED/GREEN tests for round-trip, unknown version/type, truncation, oversized length, ACK authentication failure, and correlation preservation.

### Task 2 — Legacy transport deadlines, ACK mapping, overload, and cleanup

Files: `PipeListener.cs`, `PrintReceiver.cs`, `PrintJobPayload.cs`, focused integration-style tests with loopback streams/fakes. Replace ambiguous first-int dispatch for new envelopes while preserving old frames. Add connect timeout, cancellation-aware exact reads, payload-read deadline, bounded admission, explicit encrypted overload/error ACKs, and client wait/parse of ACK with actionable status. Clear owned plaintext/encrypted buffers in `finally`; validate/sanitize document metadata before logging/history. Keep no automatic retry.

Verification: success ACK, invalid payload, offline/timeout, missing printer, spooler rejection, overload, truncated/oversized frame, cancellation, and cleanup-path tests.

### Task 3 — Bounded spooler operations and native cleanup

Files: `WindowsSpoolerAdapter.cs`, `SpoolerApi.cs`, abstractions/fakes/tests. Ensure timeout/cancellation bounds the operation contract, always closes page/document/printer handles, rejects partial writes (`dwWritten != data.Length`), and returns typed/actionable failure categories. Keep Win32 calls behind testable seams; do not claim native cancellation where the API cannot provide it.

Verification: fake spooler tests for partial write, each handle cleanup branch, timeout, cancellation, and stable error mapping; existing IPP tests remain green.

### Task 4 — Scanner deadlines and per-device admission

Files: `ScannerService.cs`, `PrintReceiver.cs`, `ScanClientService.cs`, tests/fakes. Add keyed `SemaphoreSlim` admission per scanner, validate scan parameters, bound WIA listing/transfer with a deadline, propagate cancellation/timeout to the response, and clean temporary scan files and buffers on every path.

Verification: same-scanner concurrency rejection, hung WIA timeout, cancellation, invalid parameters, successful response, failed response, and temporary-file cleanup tests.

### Task 5 — Driver package transfer safety

Files: `DriverPackageService.cs`, `PrintReceiver.cs`, `DriverPackageManager.cs`, `RealProcessRunner.cs`, tests. Validate package IDs as exactly 64 hex characters before logging/path use; serialize export/cache work per package; stream package chunks instead of loading the full 200 MB blob; bound total/idle transfer time; make process timeout kill the process tree and drain output safely; protect cache eviction during active transfer; reject duplicate/missing/out-of-order chunks.

Verification: malformed IDs, concurrent same-package requests, hung process timeout/cleanup, chunk ordering, transfer timeout/cancellation, cache eviction race, and integrity failure tests.

### Task 6 — Discovery and monitor boundedness

Files: `DiscoveryClient.cs`, `DiscoveryServer.cs`, `MonitorService.cs`, `MonitorTcpServer.cs`, tests. Add cancellation-aware discovery deadlines and bounded sweep concurrency; await receive shutdown; use immutable printer/scanner snapshots; keep monitor discovery lightweight; track listener/poll tasks; use exact async reads with cancellation; map `ProtocolError`, `AuthMismatch`, `Unreachable`, `Overloaded` distinctly; make start/stop idempotent.

Verification: no-response timeout, cancellation, bounded subnet sweep, malformed response, concurrent setters, slow driver export isolation, monitor stop during I/O, overload, and protocol/auth mapping tests.

### Task 7 — Gate 2 verification and evidence

Run focused task suites, canonical `scripts/verify-build.ps1`, diff/residue checks, and document exact counts, warnings, known hardware-only validation, and Gate 3 lifecycle inputs in `docs/production-readiness/gate-2-core-operations.md`. A PASS requires zero automated failures.

## Definition of done

- All seven tasks reviewed and committed in order.
- Legacy new clients receive encrypted ACKs with correlation IDs and actionable statuses; old payloads remain readable.
- Print, scan, driver transfer, discovery, monitor, and spooler operations have bounded deadlines/cancellation and deterministic cleanup.
- Overload is explicit; no automatic job retry is introduced.
- Full Release verification passes and evidence is auditable.
