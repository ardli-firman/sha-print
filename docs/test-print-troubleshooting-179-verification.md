# Guided troubleshooting qualification record

Issue #179 is the final assembled-flow qualification gate of #171. This record captures
the runtime evidence produced by the qualification suites, the environment they ran in,
and the criteria that remain explicitly open with the missing evidence named. Per the
issue: unavailable machine, printer, or version cases stay open; compilation and
fake-adapter success never replace physical/runtime criteria.

## Qualification suites added

1. **`apps/desktop/src-tauri/tests/guided_troubleshooting_qualification.rs`** —
   `linux_assembled_guided_troubleshooting_journey_qualifies_the_user_flow` walks the
   complete user journey through the machine daemon control boundary against a real
   isolated `cupsd`, the client proxy, and a TLS IPPS Server with a recording fake
   physical-printer adapter:
   - passive diagnosis of a healthy installed Client queue (all ten checks classified),
   - a stopped client proxy observed passively and recovered through the supervised
     runtime start path,
   - held native work (`lp -H hold`) blocking test print and repair; the fixture then
     cancels its own held job — the guided flow itself never deletes native work,
   - an explicit native test print observed end to end — CUPS Client queue → `ipp`
     backend → local proxy → TLS IPPS Server → shared printer — with the received job
     asserting `iso_a4_210x297mm`, grayscale, simplex, one copy, exactly one page, and
     `lpoptions` proving saved queue defaults unchanged,
   - physical-result confirmation recorded with the `clipping` symptom,
   - an explicit retry only after fresh checks observe the queue empty again, recorded
     as `retry-test-print` with the symptom classification cleared,
   - an observed authorization rejection: a real job submitted with a wrong Network
     Channel is rejected by the Server, the `network-channel` check fails with the
     stable `not-authorized` code, sending stays blocked until the channel is repaired
     and the observed failure card dismissed,
   - a changed Server certificate identity failing `server-identity` with
     `server-identity-changed`, blocking sending until the new fingerprint is approved
     through the trust store,
   - closure refusing stale callbacks without sending pages, and switching to a second
     installed queue superseding the session,
   - every public payload produced during the journey (session snapshots, diagnostics,
     runtime status) inspected for the generated Network Channel secrets and IPP URLs,
     together with the daemon's own log records — captured in-process with a recording
     logger whose liveness is itself asserted (the daemon start event arrives) before
     the records are trusted as a boundary,
   - persisted state compared across the stores the daemon actually uses — the Shell
     data dir (`client-server-trust.json`, `network-channel-verifier.json`, server
     identity, sharing selections) and the separate credential directory
     (`network-channel`), with the baseline asserted non-empty and containing the
     named stores: diagnosis, recovery, pending protection, native send/confirm, and
     retry leave them byte-identical; the intentional reapproval and the
     wrong-then-restored channel configuration are the only permitted diffs (trust
     store, verifier, credential), and no file is ever added.
2. **`apps/desktop/src/App.test.tsx`** — three qualification tests:
   - `qualifies the assembled journey through the visible flow and reflects it in the
     copied report` drives the visible flow end to end: stopped-proxy diagnosis, the
     explicit recovery action, native send, no-output and incorrect-output responses
     with a symptom, fresh checks, the relabeled explicit retry, and a previewed/copied
     report containing the integrated check outcome, both attempt outcomes, the
     recovery action, and the symptom — with the clipboard payload asserted equal to
     the previewed text and a generated canary in check prose asserted absent from the
     report,
   - `records a denied client-proxy recovery without unlocking the send action`,
   - `records a partially completed recovery accurately instead of claiming success`.

## Automated and command evidence

Environment: AlmaLinux 10.2 (Lavender Lion), Linux 6.12.0-211.64.1.el10_2.x86_64,
application version 3.4.4, 24 October 2026. Observed queue identities and route from
the journey run (ports are ephemeral per run):

```
[qualification] version 3.4.4 queues "ShaPrint_Office_Laser_127.0.0.1-42623_264ef502" |
"ShaPrint_Labels_127.0.0.1-42623_a712d990" route cups(127.0.0.1:33891) -> ipp backend ->
proxy(127.0.0.1:45533) -> ipps tls -> 127.0.0.1:42623
```

```bash
# apps/desktop
bun install --frozen-lockfile
bun run typecheck && bun run test

# apps/desktop/src-tauri
cargo fmt --all -- --check
cargo clippy --all-targets -- -D warnings
cargo test
cargo test --test guided_troubleshooting_qualification -- --nocapture
```

### Observed test results

1. **Frontend**: `bun run typecheck` clean; `bun run test` → 4 test files passed,
   119 tests total (assembled journey, denied and partial recovery, plus all preceding
   slice suites).
2. **Rust**: `cargo fmt --all -- --check` clean; `cargo clippy --all-targets --
   -D warnings` clean; `cargo test` → 26 suites passed with 0 failures, including
   `guided_test_print` (11), `linux_guided_test_print` (4, real CUPS),
   `desktop_control` (7), `local_troubleshooting` (15), and the new
   `guided_troubleshooting_qualification` (1, ~39 s against real CUPS).
3. **Prior art re-run as part of the full suites**: `linux_qualification` keeps the
   protocol-level four-OS-role matrix and the stable/nightly cross-channel matrix
   green; `windows_local_troubleshooting` and `windows_guided_test_print` remain
   compiled-for-Windows smoke gates (`#[ignore]`, isolated elevated runner required).

## Coverage against the qualification criteria

| Criterion | Evidence |
|---|---|
| Complete visible flow with genuine native Client queues | Linux native journey test (real CUPS queue → proxy → TLS Server); UI journey test (visible flow) |
| OS/app versions, queue identities, routes, commands, outcomes recorded | Environment block and observed route line above; per-run `println!` in the journey test; received-job and `lpoptions` assertions |
| Integrated recovery outcomes | Recovery success at native level (journey); denied and partially-completed outcomes through the visible flow (new UI tests) |
| Pending/unknown job protection | Pending jobs at native level (held job blocks send/repair); unreadable pending state at the deterministic seam (`guided_test_print`, `retry_is_blocked_by_newly_busy_unknown_queue_or_changed_trust`) |
| Changed identities, observed authorization rejection | Native journey (both block sending; reapproval and repair+dismissal restore) |
| Passive timeout/partial results, uncertain submission, delayed output | Deterministic seam and UI: `App.test.tsx` deadline tests, `guided_test_print` uncertainty/timeout/late-callback tests, delayed-output guidance assertions. Not reproduced at native level (not deterministically forceable against real CUPS) |
| Explicit retry, closure, stale callbacks, target switching | Native journey + UI journey + `guided_test_print` |
| Report reflects integrated outcomes; protected-data boundaries | UI journey asserts the copied report (incl. canary exclusion); native journey inspects every payload and the captured daemon log records (live capture asserted) for the generated secrets and IPP URLs; persisted stores compared with intentional diffs accounted |
| Stable↔Nightly and downgrade state preservation | No persisted-schema change: the real state and credential stores stay byte-identical through diagnosis/send/confirm/retry, with only the user-driven reapproval and channel reconfiguration rewriting their own stores and no files added; protocol-level cross-channel matrix green in `linux_qualification`. Real two-binary channel matrix remains open (below) |
| Repository-required checks with observed output | Above |
| No breaking changes | Confirmed: qualification adds tests and this record only; no IPC, daemon, port, wire, path, TLS, or persisted-state change |

## Explicitly open criteria (missing evidence named)

1. **Windows Client native flows** (Windows→Windows, Windows→Linux, Linux→Windows with
   a genuinely installed Windows Client queue): require an isolated elevated Windows
   runner. `windows_local_troubleshooting.rs` and `windows_guided_test_print.rs` are
   prepared gates and were not executed here.
2. **Physical paper output on both Client platforms**: requires real printers. The
   software pipeline is verified to the recording fake physical-printer adapter only;
   grayscale/one-page/simplex/media assertions cover the requested attributes, not ink
   on paper. The manual physical checklists in the #175/#176 verification records
   remain the required evidence.
3. **Real Stable/Nightly binary matrix** (Stable Client ↔ Nightly Server and
   Nightly→Stable downgrade with dispatched builds): requires dispatching both
   channels. Evidence today is the unchanged wire contract, the protocol-level
   cross-channel matrix in `linux_qualification`, and the byte-identical persisted
   state across the journey; compatibility with a real prior Stable binary is inferred
   from those unchanged contracts, not observed directly.
4. **Native-level uncertain submission, delayed output, and passive timeout**: covered
   deterministically at the application seam and in the UI (see coverage table); not
   reproduced against real CUPS, where forcing them is not deterministic.

## Compatibility

No breaking changes are introduced by this qualification slice:
- **Cross-Version Printing Compatibility (Stable ↔ Nightly)**: unchanged. Dedicated
  ports (`48631` IPPS Server, `48632` Client Proxy, `48633` Discovery), IPPS wire
  protocol, Network Channel authentication headers, queue URI paths
  (`/ipp/print/...`), and TLS identity/fingerprint storage are untouched.
- **Settings & State Backward Compatibility**: the qualification shows the guided flow
  neither mutates the real persisted stores (state and credential directories, with
  only the user-driven reapproval and channel reconfiguration rewriting their own
  stores) nor adds a persisted schema, and no storage format changed in this slice, so no `SETTINGS_RISK=true` dispatch is required;
  direct two-binary downgrade evidence remains open as recorded above.
