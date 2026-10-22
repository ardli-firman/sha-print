# Local Client queue diagnostics verification

Scope: ticket #172 under spec #171. This implements passive local diagnostics,
not the subsequent Server diagnostics, print submission, guided recovery, or
report-export tickets.

## Observed checks

- `bun run typecheck && bun run test` from the workspace: passed. Desktop tests:
  90 passed across four files. Landing tests: 24 passed across three files.
- `cargo fmt --all -- --check`: passed.
- `cargo clippy --all-targets -- -D warnings`: passed with the desktop feature.
- `cargo test`: passed, 541 tests passed, zero failed, four ignored. The ignored
  native CUPS test was also run explicitly as described below. Other ignored
  platform/physical-printer checks are not claimed as executed.
- `cargo test --test local_troubleshooting --test desktop_control`: passed,
  including public application/control behavior for queue evidence, protected
  unknown job state, partial results within the 15-second overall deadline, and passive
  reads over the real daemon socket without invoking elevation.
- `cargo check --no-default-features --lib --target x86_64-pc-windows-gnu`:
  passed. A temporary user-local MinGW compiler was used; no system compiler
  package was installed.
- `cargo check --all-targets --target x86_64-pc-windows-gnu`: passed with the
  desktop feature, including the new Windows native smoke-test code.

Rust commands above run from the desktop Rust crate with the native desktop
prerequisites. Cross-checks included the temporary MinGW directory in `PATH`.

## Linux native runtime exercise

`cargo test --test local_troubleshooting_native -- --ignored --nocapture` passed:
one test passed, zero failed. It starts an isolated unprivileged CUPS scheduler
and a real ShaPrint daemon, installs a genuine native Client queue, and inspects
it through the desktop machine control plane over the Unix socket.

The observed states were a matching empty queue with a running client proxy,
held native print work, a stopped client proxy, a mismatched queue destination,
and a missing queue. Repeated inspection preserved the held print work. The
test generates that work at runtime in the isolated spooler and does not send
anything to a physical printer or modify the host's installed printer queues.

The native inspection budget reserves one second of the 15-second overall
deadline for local IPC delivery. The backend finalizes its partial snapshot by
14 seconds. The UI finalizes an unresponsive run at 15 seconds and ignores later
success or failure until an explicit rerun; a returned authoritative snapshot
retains completed evidence and inconclusive unfinished checks.

Visible-flow tests render the application through its public IPC boundary and
exercise opening the selected queue, pending-work guidance, the UI deadline,
explicit rerun, target switching, closing, and rejection of superseded results.
The troubleshooting panel offers no test-print or repair action in this slice.

## Windows native runtime exercise

Desktop CI now explicitly provisions the disposable Windows runner's print
services and runs `windows_native_diagnostics_are_self_contained` with `--ignored`.
That smoke test creates its own IPPS Server, proxy, and genuine native IPP Client
queue, generates held work only in the paused temporary queue, and observes the
public desktop control results for healthy, pending, stopped-proxy, mismatched,
missing, and unavailable-spooler cases. It asserts no work reaches the fake
physical printer. Rust cleanup guards and an unconditional CI cleanup step remove
the uniquely named fixture queue and restore the runner's spooler.

[Desktop CI run 39700898349](https://github.com/ardli-firman/sha-print/actions/runs/39700898349)
passed on Windows at commit `eb20ed015e876f4c12bd51e584c6b980e523ed54`.
The explicit native smoke test reported:

```text
[native-diagnostics] matching empty queue, spooler and proxy: passed
[native-diagnostics] repeated inspection preserves held work: passed
[native-diagnostics] stopped proxy: passed
[native-diagnostics] mismatched destination: passed
[native-diagnostics] missing queue and explicit cleanup: passed
[native-diagnostics] unavailable spooler and passive-only diagnostics: passed
test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 14.05s
```

The print-service preparation and unconditional fixture cleanup steps also passed.
The Windows Rust format, clippy, and ordinary test suite and the frontend job
passed in the same run. This is native runtime evidence through the public
ControlPlane; no launched interactive Windows desktop UI is claimed.

## Additional manual qualification

Interactive Windows UI checks remain useful for the broader qualification in
#179. The rendered application tests cover the visible session behavior for this
slice; the native CI test exercises the Windows command and print services.

On a Windows machine with an installed ShaPrint Client queue, set
`SHAPRINT_WINDOWS_DIAGNOSTICS_QUEUE` to its exact queue name and run
`cargo test --test local_troubleshooting windows_native_queue_diagnostics -- --ignored --nocapture`.
Also open **My printers > Troubleshoot** and verify healthy, pending-work,
unavailable-spooler, and stopped-proxy results, rerun, and session closure. The
smoke test itself performs passive native enumeration only; it neither prints
nor repairs a queue.

Mixed Stable/Nightly runtime printing, downgrade exercises, and the full
cross-platform physical-output matrix were not rerun for this local-only slice.
They remain explicit qualification criteria under #179. No printing/state
compatibility result is inferred merely from unchanged types or compilation.

## Compatibility

No breaking changes: existing dedicated ports, IPPS wire protocol, Network
Channel authentication headers, queue URI paths, and TLS identity/fingerprint
storage are unchanged. Stable Clients can continue printing to Nightly Servers
and Nightly Clients to Stable Servers under the existing supported-version
assumptions. Persisted settings, Trusted Servers, shared printer selections, and
Network Channel storage remain backward-compatible on downgrade. The added
local read-only daemon request introduces no persisted-state migration and
requires no `SETTINGS_RISK=true` dispatch.

## Standards review

Zero outstanding documented-standard violations or actionable baseline smells.
The review checked application/adapter/IPC boundaries, blocking native work,
classified public messages, and passive Linux reads without elevation.

## Spec review

Zero outstanding code findings after reviewing the finalized deadline behavior.
The backend reserves local IPC delivery time, and the UI ignores late success or
failure after settlement. Tests cover retained partial evidence, explicit rerun,
superseded sessions, and late responses. Windows native runtime passed in the
linked CI run above. Interactive Windows UI and mixed-version physical
qualification are not claimed by this record.
