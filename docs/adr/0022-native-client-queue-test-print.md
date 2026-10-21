# Native Client queue test prints and user-confirmed output

---
status: accepted
extends: ADR 0008, ADR 0019
---

Troubleshooting targets a Client queue that previously worked. Submit its test
print through the installed native Client queue so the test exercises the path
used by other applications. A direct IPPS submission could succeed while leaving
a local queue or driver problem undetected. Supporting both Windows and Linux
native paths costs more than a direct network-only test, but provides evidence
about the path the user needs to repair.

Represent spooler acceptance separately from physical output. Existing backend
completion means the platform submitter succeeded; it does not prove that paper
was produced correctly. Ask the user whether the diagnostic page printed
correctly, did not print, or printed with a problem. An unanswered or closed
session leaves physical output unconfirmed. This adds user interaction in
exchange for avoiding a misleading physical-success claim.

The user confirmed these decisions and the complete feature design in Q21 of
the design interview. Implementation is tracked under #171; full-feature runtime
qualification remains pending.
Detailed workflow decisions are tracked in the
[design document](../test-print-troubleshooting-design.md).

This document introduces no breaking changes. Stable Client to Nightly Server
printing and the reverse direction are unchanged, as are persisted settings,
Trusted Servers, shared printer selections, and Network Channel storage on
downgrade. The intended implementation preserves existing ports, IPPS wire
protocol, Network Channel authentication headers, queue URI paths, and TLS
identity/fingerprint storage; that compatibility still requires verification.
