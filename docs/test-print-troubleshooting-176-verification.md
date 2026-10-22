# Linux guided test print verification

Issue #176 implements the Linux send slice of #171 and ADR 0022. Native CUPS queue
inspection and submission have been verified in end-to-end integration tests with real
isolated `cupsd` instances and TLS servers. Physical hardware output verification
remains open.

## Implemented behavior

- Linux troubleshooting reads the selected native CUPS Client queue's default Requested media
  via destination options, PPD (`cupsGetPPD2`), and `lpoptions`. Missing, unsupported, or
  unreadable media blocks sending without choosing a substitute size.
- An explicit send rechecks native CUPS destination state, the client proxy, live Server trust,
  selected shared printer, and locally observed authorization. The native submission worker
  checks destination existence, accepting status, stopped state (`IPP_PSTATE_STOPPED`),
  active job count (must be zero), matching proxy URI, and default media again before creating a job.
- The Linux adapter (`LinuxNativeTestPrint`) draws one diagnostic page into a standard PWG Raster
  stream (`image/pwg-raster`, sgray_8, 300 DPI). It requests one copy, grayscale, one-sided printing,
  and the observed default media without mutating saved queue defaults. The page identifies
  Requested media, displays borders at 2, 5, and 10 mm insets, a 50 x 50 mm center alignment cross,
  and diagnostic text.
- Diagnostic page submission submits directly to the native CUPS Client queue using dynamic
  `libcups.so.2` bindings (`cupsCreateJob`, `cupsStartDocument`, `cupsWriteRequestData`,
  `cupsFinishDocument`, and `cupsCancelJob2`). Once a job is created in CUPS, streaming or
  finalization errors cancel the CUPS job and report `Uncertain` without retrying or canceling
  downstream jobs.
- Session state distinguishes blocked/rejected/accepted/uncertain submission from
  unconfirmed/correct/no-output/incorrect physical results. The user supplies the physical
  result via the desktop window. A 30-second native submission timeout is treated as uncertain.
- Diagnostic inspection and test print submission are unprivileged user actions: neither
  invokes `pkexec` or escalates privileges.
- All test print session operations (`open`, `refresh`, `send`, `confirm`, `close`) are wired
  through the machine daemon socket (`ControlRequest` / `ControlResponse` in `adapters/daemon.rs`),
  supporting desktop clients running in `ControlPlane::Machine` mode.
- Closing discards session state and ignores late callbacks without canceling in-flight work.
  A subsequent send requires fresh prerequisites.
- Public snapshots contain classified checks, stable codes, catalog media, attempted
  submission outcomes, and physical-result enums. Native error text and document content
  are never exposed. There is no new persisted session schema.

## Automated and command evidence

Environment: Linux x86_64, application version 3.4.4, 22 October 2026.
Agreed primary seams: `NativeTestPrint` trait, `GuidedTestPrint` application orchestrator,
Tauri IPC commands, daemon socket RPC, and the UI in `LocalTroubleshootingDialog.tsx`.

```bash
# apps/desktop
bun run typecheck && bun run test

# apps/desktop/src-tauri
cargo fmt --all -- --check
cargo clippy --all-targets -- -D warnings
cargo test
```

### Observed test results

1. **Frontend**:
   - `bun run typecheck`: clean, zero TypeScript errors.
   - `bun run test`: 4 test files passed (103 tests total: `availability.test.ts`, `types.test.ts`, `version.test.ts`, `App.test.tsx`).
2. **Rust Backend**:
   - `cargo fmt --all -- --check`: clean, zero diffs.
   - `cargo clippy --all-targets -- -D warnings`: passed with warnings denied, zero warnings or errors.
   - `cargo test`:
     - `shaprint_desktop` unit tests: 407 passed, 0 failed.
     - `linux_guided_test_print`: 4 passed, 0 failed (covers direct queue print without mutating defaults, print through `shaprintd` daemon socket over Unix domain socket, rejection/disablement on stopped or mismatched queues, and rejection/disablement on unsupported media).
     - `desktop_control`: 7 passed, 0 failed (verifies daemon socket reads, 10 diagnostic checks including `requested-media`, and test print rejection when not ready).
     - `guided_test_print`: 8 passed, 0 failed (platform-agnostic session orchestration).
     - `cups` adapter tests: passed against real `cupsd`.
     - Full integration suite: all 24 integration test crates passed cleanly.

## Review and Refactor Notes

During the independent Standards and Spec code reviews:
1. **Standards review**:
   - Verified zero secret/credential leakage in DTOs, logs, or test fixtures.
   - Identified repeated physical result string parsing across `ipc/control.rs` and `adapters/daemon.rs`. Addressed by adding `PhysicalResult::parse` to parse and validate physical result strings in one central domain method.
   - Identified repeated `lpoptions` subprocess construction in `read_default_media`. Addressed by extracting `lpoptions_cmd`.
2. **Spec review**:
   - Identified proxy port resolution in `src/lib.rs` where `ports.proxy_port == 0` could produce `"127.0.0.1:0"` instead of `CLIENT_PROXY_DEFAULT_PORT` (`48632`). Fixed to use `CLIENT_PROXY_DEFAULT_PORT`.
   - Identified that `lpoptions` fallback in `linux_test_print.rs` omitted `-h` when host was a UNIX socket path. Fixed `lpoptions_cmd` to always pass `-h <endpoint.lpadmin_host_arg()>`.
   - Identified missing unsupported-media test on Linux route. Added `linux_guided_test_print_disabled_on_unsupported_media` verifying that unsupported media disables test print without silent replacement.

## Open physical criteria

Automated tests verified the entire software pipeline: drawing the PWG Raster page, submitting to an
isolated CUPS daemon, forwarding through the native Client queue via the `ipp` backend to the local
proxy, authenticating to the IPPS TLS server with the Network Channel, and verifying received job
attributes (1 page, 300 DPI, monochrome, simplex, correct media keyword).

Required manual evidence with physical hardware:

1. On a Linux Client connected to a remote ShaPrint Server (Windows or Linux), open
   My printers > Troubleshoot for the installed Client queue.
2. Confirm the default Requested media is detected and displayed.
3. Choose "Send test print" and verify that one physical page is produced.
4. Verify physical output: grayscale, one-sided, single copy, 2/5/10 mm insets, 50 mm center cross.
5. Confirm "It printed correctly" and verify that saved CUPS queue options (`lpoptions -p <queue>`)
   were not altered.

## Compatibility

No breaking changes are introduced:
- **Cross-Version Printing Compatibility (Stable ↔ Nightly)**: A Stable Client can print to a Nightly
  Server, and a Nightly Client can print to a Stable Server. Dedicated ports (`48631` IPPS Server,
  `48632` Client Proxy, `48633` Discovery), IPPS wire protocol, Network Channel authentication headers,
  queue URI paths (`/ipp/print/...`), and TLS identity/fingerprint storage remain unchanged.
- **Settings & State Backward Compatibility**: Persisted settings, Trusted Servers, shared printer
  selections, and Network Channel storage remain completely backward-compatible on downgrade. No state
  migration or `SETTINGS_RISK=true` is required.
