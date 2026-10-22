# Windows guided test print verification

Issue #175 implements the Windows send slice of #171 and ADR 0022. Native Windows
runtime and physical-output acceptance remain open. Automated adapter tests and
cross-compilation do not establish that a Windows spooler produced a page.

## Implemented behavior

- Windows troubleshooting reads the selected Client queue's default Requested media.
  Missing, unsupported or unreadable media blocks sending without a replacement size.
- An explicit send rechecks native queue state, the client proxy, live Server trust,
  selected shared printer and locally observed authorization. The native worker checks
  destination, pending jobs and media again before starting a document. Concurrent
  applications can still enqueue; this does not reserve the native queue.
- The Windows adapter draws one diagnostic page through the installed Client queue's
  GDI driver. It requests one copy, grayscale, one-sided printing and the observed
  default media without saving queue preferences. The page identifies Requested media,
  shows borders at 2/5/10 mm and a 50 mm center alignment cross.
- Session state distinguishes blocked/rejected/accepted/uncertain submission from
  unconfirmed/correct/no-output/incorrect physical results. The user supplies the physical
  result. A 30-second native-submission timeout is uncertain and never retries or cancels.
  The outstanding worker blocks further sends across sessions until it actually finishes.
- Closing discards session results and ignores late callbacks. It does not cancel work.
  Another send requires fresh checks and includes a downstream duplicate-output warning.
- Linux retains diagnostics and recovery but has no send action or daemon send operation.
  The application session orchestration is shared for the later Linux adapter in #176.
- Public snapshots contain classified checks, stable codes, catalog media, attempted
  submission outcomes and physical-result enums. Native messages and page content cannot
  enter the new session fields. There is no new persisted session schema.

## Automated and command evidence

Environment: AlmaLinux 10.2 x86_64, application version 3.4.4, 22 October 2026.
The agreed primary seam is the application use cases and desktop control/typed IPC.
The UI exercises that IPC contract through visible controls in jsdom.

```bash
# Repository root
bun install --frozen-lockfile
bun run typecheck && bun run test

# apps/desktop, after the final UI status-message adjustment
bun run typecheck
bun x vitest run src/App.test.tsx

# apps/desktop/src-tauri
cargo fmt --all -- --check
cargo clippy --all-targets -- -D warnings
CARGO_BUILD_JOBS=2 cargo test
```

Observed frontend results: workspace typecheck passed; desktop 103 tests passed in
4 files; landing 24 tests passed in 3 files. The final focused App suite passed
85 tests. The first typecheck attempt preceded dependency installation and failed
for missing packages. One later focused UI run timed out starting its worker while
Rust linkers exhausted available memory; it passed after limiting Rust build jobs.
The unrestricted full Rust test build was interrupted during linking for the same
resource pressure. The bounded Rust run passed 567 tests with 0 failures and
4 ignored tests across 24 reported suite results. Format and Linux desktop Clippy
also passed with warnings denied.

The feature suite exercises explicit send, pending/unknown/missing/mismatched queue
state, unreachable/timed-out/untrusted/changed Server identity, unshared printer,
missing/changed media, accepted/rejected/uncertain submissions, physical responses,
duplicate sends, late output after timeout, closure/reopening and generated protected
native error data. Desktop control also runs the actual Linux daemon and confirms
that send is unsupported without invoking the privileged helper. Existing passive
checks remain covered by `local_troubleshooting`.

Windows target verification uses a temporary Zig 0.14.1 C compiler/archive wrapper
and AlmaLinux's extracted mingw64-binutils 2.44 for resource compilation. These tools
live outside the repository; no workspace dependencies or system packages changed.

```bash
# Repository root, with those temporary wrappers on PATH
CC_x86_64_pc_windows_gnu=/tmp/shaprint-175-cc \
AR_x86_64_pc_windows_gnu=/tmp/shaprint-175-ar \
cargo clippy --manifest-path apps/desktop/src-tauri/Cargo.toml \
  --target x86_64-pc-windows-gnu --no-default-features --all-targets -- -D warnings

PATH="/tmp/usr/bin:$PATH" \
CC_x86_64_pc_windows_gnu=/tmp/shaprint-175-cc \
AR_x86_64_pc_windows_gnu=/tmp/shaprint-175-ar \
cargo check --manifest-path apps/desktop/src-tauri/Cargo.toml \
  --target x86_64-pc-windows-gnu --all-targets
```

Both checks passed. The initial desktop target check reported two unused shutdown
results in the new smoke test; they were changed to asserted successful results.
Final desktop Windows Clippy also passed for all targets with warnings denied,
using the same environment and `cargo clippy --target x86_64-pc-windows-gnu
--all-targets -- -D warnings`. The smoke test is compiled but remains unexecuted.

## Open Windows runtime and physical criteria

Run on an isolated elevated Windows runner with Microsoft IPP Class Driver:

```powershell
cd apps/desktop/src-tauri
cargo test --test windows_guided_test_print -- --ignored --nocapture
```

This fixture installs a uniquely named native Client queue, sends through the public
control boundary, waits for the real local proxy and TLS Server to reach a fake
physical-printer adapter, and compares saved queue defaults before/after. It has not
been executed in this Linux environment. It cannot establish physical output.
The existing `windows_local_troubleshooting` fixture remains passive; its old
assumption that a healthy queue can never offer send has been updated.

Required manual evidence, still open:

1. On Windows Client to Windows Server and Windows Client to Linux Server, record OS
   versions, queue identities, selected shared printer and observed native route.
2. With a real printer, open My printers > Troubleshoot, inspect Requested media,
   choose Send test print once, then verify one grayscale, one-sided page and one
   copy. Compare the page's media identification and patterns with Loaded media.
   Record unchanged native queue defaults and the user's physical confirmation.
3. Exercise delayed output, pending work, unknown queue state, panel closure while
   submitting, and explicit retry on Windows. Observe that closure does not cancel
   and that prior work can still print downstream.
4. Record exact Stable/Nightly versions for both Client/Server directions and test
   Nightly-to-Stable downgrade preservation. These live version/state cases have
   not been run for this change.

## Compatibility

No breaking changes are introduced. Stable Client to Nightly Server and Nightly
Client to Stable Server retain the existing print contracts. Ports 48631/48632/48633,
IPPS, Network Channel headers, `/ipp/print/...` paths, and TLS identity/fingerprint
storage are unchanged. Persisted settings, Trusted Servers, shared printer selections
and Network Channel storage remain backward-compatible on downgrade. No state migration
or `SETTINGS_RISK=true` dispatch is needed. The live version/state matrix above is still
open; unchanged contracts are not claimed as runtime evidence.

## Standards

The independent standards review found one unmet verification gate: `AGENTS.md`
requires the changed Windows path to be exercised, and its native/physical runtime
checks remain open. No other documented-standard violation or protected-data,
wire-contract or persisted-state change was found.

An optional duplicated-classification suggestion was addressed by sharing the private
proxy and Network Channel classification helpers between inspection and the immediate
pre-send refresh. The reviewer confirmed unchanged classification precedence and no
new findings. After this review refactor, the 8 guided-test-print tests and 15 local
troubleshooting tests passed again, as did Linux desktop Clippy with warnings denied
and `cargo fmt --all -- --check`.

## Spec

The independent specification review found no additional implementation defect or
scope creep in #175. It confirmed separate submission/physical results, fresh send
checks, duplicate protection, closure behavior, stale-session isolation, and Linux's
lack of a send action. The native Windows route, physical-page evidence and live
version/downgrade qualification remain partial acceptance criteria. #175 is not
claimed as fully runtime-qualified.

Review totals: Standards has 1 open verification gate and 0 remaining implementation
findings; Spec has 1 open acceptance-evidence finding and 0 implementation defects.
