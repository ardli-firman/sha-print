# Guided test print and troubleshooting

Status: accepted. The user confirmed D1 through D20 and approved the consolidated
workflow and acceptance criteria in Q21. Implementation is tracked under #171;
the first local-diagnostics slice is #172. Full-feature acceptance criteria below
remain pending until all slices and runtime qualification are complete.

## Confirmed decisions

- D1: Focus the first release on a Client queue that previously printed successfully
  and now fails to produce output. Post-installation verification is secondary.
- D2: Support Windows and Linux desktop in the first release. Android is outside
  this release's scope.
- D3: Offer specific recovery actions for the user to invoke. Checks do not
  automatically perform repairs. Sending a test page is a separate user action.
- D4: Submit the test print through the installed native Client queue, exercising
  the path used by other applications. Direct IPPS submission is not a substitute
  for this test.
- D5: After submission, ask whether the diagnostic page printed correctly, did
  not print, or printed with a problem. Physical output remains unconfirmed until
  the user responds. Spooler acceptance alone does not establish physical output.
- D6: Provide entry points on installed Client queues and on print failure cards
  when their target Client queue can be identified.
- D7: Start with passive diagnostics. Offer a test print or a relevant recovery
  action after showing the findings. Passive checks do not request administrator
  authorization, consistent with ADR 0020.
- D8: Block test print submission while the selected Client queue has pending
  print jobs. Guide the user to inspect the native operating-system queue;
  troubleshooting does not delete those print jobs.
- D9: Use the Client queue's default media size and display it before submission.
  The diagnostic page identifies the Requested media and includes alignment and
  clipping patterns. This test does not qualify every media choice in other
  applications. Incorrect output can lead to comparing Requested media with
  Loaded media.
- D10: Use one diagnostic page, one copy, grayscale, and one-sided printing.
  The first release does not verify color or duplex behavior.
- D11: First-release recovery actions are starting the client proxy, repairing
  the Client queue, and directing the user to existing Network Channel and
  Server identity review flows. Operating-system spooler and firewall issues
  receive inspection instructions rather than new direct mutation actions.
- D12: Resubmission requires an explicit user action after diagnostics are rerun
  and the Client queue is empty. Warn that the earlier print job may still
  produce output downstream. There is no automatic retry.
- D13: Results remain session-only, with a previewable report that the user may
  copy. Include check outcomes, stable error codes, application version, and
  actions attempted. Exclude Network Channel values, document names, and print
  job content. No persistent troubleshooting history is added in this release.
- D14: The user may close the panel before confirming physical output. Explain
  that closing does not cancel submitted print jobs. The physical result remains
  unconfirmed, and the session-only results follow D13.
- D15: Allow one active troubleshooting session. Starting one for another Client
  queue requires finishing or closing the current session.
- D16: For incorrect physical output, offer symptom selection such as clipping,
  shifted position, incorrect size, and blank output, followed by relevant checks.
  These symptoms guide investigation; they do not establish a cause by themselves.
- D17: If the Client queue's pending-job state cannot be established, block both
  test print and repair. Unknown queue state is not equivalent to an empty queue.
- D18: Direct cancellation to the native operating-system queue interface in the
  first release. There is no troubleshooting-panel cancellation action. Explain
  that cancelling locally may not stop a print job already forwarded downstream.
- D19: Use locally observable state and existing Server responses. Add no remote
  Server diagnostics API. State the visibility limit when evidence is missing;
  inability to reach a Server does not establish a firewall fault.
- D20: Give each passive diagnostic run an initial overall deadline of 15 seconds.
  Show completed checks and mark unfinished checks inconclusive. Offer an explicit
  rerun; incomplete essential checks continue to block the actions they guard.

The accepted architectural rationale for D4 and D5 is recorded in
[ADR 0022](adr/0022-native-client-queue-test-print.md).

## Existing behavior that constrains the design

- Linux diagnostics inspect daemon, ports, CUPS, trust, credential configuration,
  and firewall. Windows currently reports its in-process desktop runtime and the
  latest failure; diagnostic depth differs between the platforms. See
  [control.rs](../apps/desktop/src-tauri/src/ipc/control.rs) and
  [ADR 0019](adr/0019-safe-linux-diagnostics-and-journal-events.md).
- A locally configured Network Channel does not prove that a particular Server
  accepts it. Findings must distinguish local configuration from observed
  authorization results.
- Direct IPPS submission bypasses the installed Client queue. Exercising that
  queue requires using the operating system's native print path. Linux has a
  native Client queue test precedent in
  [linux_daemon.rs](../apps/desktop/src-tauri/tests/linux_daemon.rs).
- Managed IPP job state becomes completed when the platform submitter succeeds.
  This proves submission to the spooler, not physical output. See
  [endpoint.rs](../apps/desktop/src-tauri/src/adapters/ipps/endpoint.rs).
- Existing queue repair protects pending print jobs; Windows repair removes and
  recreates the matching Client queue after its checks. See
  [windows.rs](../apps/desktop/src-tauri/src/adapters/queue_installation/windows.rs)
  and [cups.rs](../apps/desktop/src-tauri/src/adapters/queue_installation/cups.rs).
- Diagnostics must preserve the secret and document-content boundaries in
  [ADR 0008](adr/0008-print-failure-reporting.md) and ADR 0019.

## Workflow

1. Open troubleshooting for one identified Client queue from its installed-printer
   entry or an associated print failure card. Show the target printer and Server.
2. Run passive checks automatically, with the D20 deadline. Inspect the Client
   queue and pending print jobs, local runtime and spooler, saved trust and live
   identity when reachable, Server reachability, and shared-printer availability
   through existing supported operations. Distinguish configured credentials
   from observed authorization. Platform-specific checks may provide additional
   evidence without claiming equal visibility into every subsystem.
3. Show each check as passed, failed, inconclusive, or not applicable. Derive
   explanations and recovery guidance from classified results. A generic timeout
   does not establish a wrong Network Channel, blocked firewall, or broken printer.
4. Offer the relevant D11 actions. Request any administrator authorization through
   the existing bounded setup or repair flow only after the user chooses that
   action. Denial, failure, or partial completion leaves the session open with the
   actual result; it does not establish recovery. Rerun affected checks afterward.
5. Offer test print when the selected queue is present, its pending state is known
   to be empty, essential runtime/trust/target checks permit it, and its default
   media size is usable. Display the Requested media and D10 settings before the
   explicit send action. Missing or unsupported media receives recovery guidance;
   do not silently substitute a different size. Optional inspection limits, such
   as an unavailable firewall inspection, are not themselves confirmed failures.
6. Recheck queue emptiness and relevant prerequisites immediately before submission
   or repair. Existing platform repair protections still apply. Other applications
   can enqueue work concurrently; this feature does not reserve the OS queue.
   Apply test settings to this print job without changing saved queue defaults.
7. Submit once through the native Client queue. Disable duplicate send actions
   while submission is in progress. Distinguish a definite rejection from a
   submission whose outcome cannot be established. A submission timeout does not
   prove that no print job was accepted. D20 governs passive checks, not the time
   allowed for physical output.
8. Report observable submission progress without claiming paper output. Ask for
   D5's physical result. Correct output completes the test. No output leads to
   queue/Server/printer inspection guidance. Incorrect output leads to D16's
   symptom-specific guidance. Reported symptoms do not establish remote causes.
9. An explicit retry follows D12, including fresh checks and the downstream-output
   warning. Queue emptiness alone does not prove an earlier test cannot print.
   Opening the native queue is an explicit action; cancellation follows D18.
10. Allow preview and copying of the structured D13 report. Do not add arbitrary
    document or secret-bearing fields. Close or finish the session before opening
    another. Closing discards session results without cancelling submitted work.
    Late callbacks from a closed or superseded diagnostic run cannot change a new
    session's findings or trigger submission. Reopening starts a fresh session and
    does not imply earlier submitted work was cancelled.

## Acceptance criteria

All criteria below are pending implementation and runtime verification.

- On Windows and Linux Clients, the test traverses a genuinely installed native
  Client queue, the local proxy, and a Server's selected shared printer. A direct
  HTTP/IPPS request is insufficient evidence for this acceptance criterion.
- Exercise Windows Client to Windows Server, Windows Client to Linux Server,
  Linux Client to Windows Server, and Linux Client to Linux Server paths. Record
  the OS versions, selected queues, observed route, and actual outcomes.
- Verify physical output from both Client platforms: one grayscale, one-sided
  page, one copy, expected media and alignment patterns, and no changed queue
  defaults. Record the user's physical-output confirmation. Spooler acceptance
  alone cannot satisfy this criterion.
- Reproduce a stopped client proxy, missing or mismatched Client queue, pending
  jobs, unreadable pending state, unreachable Server, changed certificate identity,
  unshared printer, and an observed authorization rejection. Findings must name
  the evidence available and preserve the appropriate submission/repair gates.
- Verify the passive deadline with partial results. No administrator prompt,
  automatic print, automatic repair, or automatic retry occurs. A timed-out or
  unavailable check is not labelled passed or assigned an unsupported cause.
- Verify repair success, declined elevation, repair failure, and a queue becoming
  busy before repair. Existing print jobs remain protected. Session results name
  the observed outcome and affected checks are refreshed before another action.
- Verify delayed output, uncertain submission, no output, incorrect output,
  explicit retry, closing during a run or submission, late callbacks, and opening
  another printer's troubleshooting. No duplicate send is generated by UI events;
  uncertain submission and retry retain their downstream-output warnings.
- Inspect IPC, diagnostics, logs, report preview, and copied report for protected
  data boundaries. Test page bytes, document names/content, Network Channel values,
  credentials, and key-bearing IPP URLs do not enter diagnostic/report payloads.
- Exercise Stable Client to Nightly Server and Nightly Client to Stable Server
  paths, and downgrade state preservation. Record versions and observed outcomes;
  do not infer compatibility from compilation or unchanged API shapes alone.
- Run the repository-required frontend and Rust suites for the changed files,
  recording exact commands and observed results alongside runtime evidence.

## Decision tree

The design branches are resolved by D1 through D20: scope, entry points, passive
checks, native submission, output confirmation, recovery, retries, session/report
lifecycle, uncertainty, cancellation, and remote visibility.

Q21 confirmed that the consolidated workflow and acceptance criteria match the
shared understanding. The design interview is complete with no open decision
branches. ADR 0022 is accepted; implementation and verification are separate work.

## Compatibility

This document introduces no breaking changes or runtime/state changes. It leaves
Stable Client to Nightly Server printing, Nightly Client to Stable Server
printing, and downgrade compatibility unchanged. Persisted settings, Trusted
Servers, shared printer selections, and Network Channel storage are unchanged.

The implementation must preserve the existing dedicated ports, IPPS
wire protocol, Network Channel authentication headers, queue URI paths, and TLS
identity/fingerprint storage. Implementation compatibility remains to be verified.
Any proposal that changes those contracts must be explicitly identified as
breaking before it is accepted.
