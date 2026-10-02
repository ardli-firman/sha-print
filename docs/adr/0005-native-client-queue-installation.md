# Native client queue installation

---
status: accepted
---

A client user selects a printer a trusted server is sharing and installs a native Windows queue for
it (issue #35). The queue is what makes a shared printer reachable from ordinary Windows print
dialogs (ADR 0001), so its destination, its name, and the permission it needs are all part of the
product contract.

**Destination.** The queue's port is an IPP URI on loopback:
`ipp://127.0.0.1:8632/ipp/print/{encoded server}/{encoded printer}`. The local client proxy already
parses exactly that shape and rewrites it to the approved server's IPPS URI while adding the
Network Channel (#34), so one builder (`adapters::client_proxy::client_queue_uri`) defines the
destination for the platform installers, and the integration test drives the proxy through the same
builder. Test doubles elsewhere report a deliberately implausible stub URI, so no test asserts a
second, divergent shape. The proxy port is the product's fixed `CLIENT_PROXY_DEFAULT_PORT`; the
helper never accepts an authority from its command line, so a caller cannot point a queue at an
arbitrary host.

**Creation.** Windows creates the queue with `Add-Printer -Name <queue> -IppURL <url>`, the same
command the client-proxy native smoke test uses. The queue name and URL travel through environment
variables and are never interpolated into the script text, so a printer name cannot change what
PowerShell executes.

Creation never removes a queue: re-running the install after a restart is the common case, and it
must leave a working queue alone. Because the name is derived from the printer and the server, a
queue carrying that name already points at the destination being installed, so the script reports
success without touching it. If the name is taken by a queue pointing somewhere else, the script does
not take it over either — it reports `existing-queue-conflict`, and the app tells the user to remove
that printer in Windows and install again. Removing a queue stays a manual action in Windows'
printer settings.

**Name.** The name is derived, not supplied: `"<printer> (ShaPrint <label>)"`, where the label is
the canonical `host:port` address with the punctuation a printer name cannot carry replaced. The
same shared printer therefore always maps to the same local queue, and two servers sharing a
same-named queue stay distinct. Names the spooler cannot accept (over 220 characters, control
characters, `\` or `,`) are refused with the action that fixes them.

**Permission.** Installing a spooler queue is machine-wide state, so it joins inbound firewall
access as the second — and last — action classified as requiring elevation (ADR 0001). Everything
else a client user does stays unprivileged: reviewing a fingerprint, listing shared printers, and
choosing which printer to install. Every precondition is settled *before* the prompt: the server
must be explicitly approved and must currently share that printer, and the local proxy must be
running. A queue is never created for a destination that cannot work.

**Failure reporting.** The helper is a separate process started through a UAC prompt, so it has no
standard streams to answer on. It reports one classified reason through its exit code
(`SetupFailureKind`, codes 10–17, above the C runtime's abnormal-termination codes) and keeps
Windows' own localized text on standard error for diagnosis. Only the classification crosses the
process boundary: the app composes the message the user reads, naming the queue and repeating what
they can do about it. An unclassified exit code — a crash, or a malformed request — becomes `other`
rather than being mistaken for a classified failure. The parent bounds its wait for the helper and
terminates one that overstays, reporting `timed-out` instead of hanging the app.

## Consequences

- The installed queue outlives the app, and the client proxy autostarts, so a shared printer stays
  usable across ShaPrint restarts. Printing while ShaPrint is closed fails, because the proxy is
  what supplies the Network Channel.
- Installing does not remove a queue, and there is no uninstall action yet: a user removes the queue
  from Windows' own printer settings.
- Only Windows 10 and later can install a queue; other platforms report `unsupported` before any
  prompt.
- `Add-Printer -IppURL` needs the PrintManagement module, which ships with the supported Windows
  versions; the script reports `unsupported` when the command is missing rather than failing
  obscurely.
- The URI, the name derivation, the command construction, and the failure classification are all
  verified on every platform, including in continuous integration. Creating a queue in a real spooler
  and printing through it is exercised only by the ignored Windows smoke test
  (`tests/queue_installation.rs::windows_installs_a_native_queue_that_prints_through_the_proxy`),
  which needs an elevated session and the product's own ports.
- The elevated helper re-validates the address and printer it is handed but does not consult the
  trust store: it cannot, because it runs without the app's state. The destination is hard-wired to
  loopback, and the proxy re-checks the approval on every job, so the worst a crafted helper request
  can do is install a queue that the proxy then refuses to serve.
