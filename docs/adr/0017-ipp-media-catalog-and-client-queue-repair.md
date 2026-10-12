# IPP media catalog and client queue repair

---
status: accepted
---

ShaPrint publishes one fixed media catalog for every Shared printer. The catalog is the source for
IPP `media-supported` and `media-ready`, PWG Raster dimension matching, and Windows `DEVMODE`
mapping. ShaPrint no longer advertises `custom_min_*`, `custom_max_*`, or custom dimension ranges.
This prevents Microsoft IPP Class Driver from presenting paper sizes that the Server cannot map to
the selected Windows queue. Per-printer capability intersection remains out of scope because the
Server does not have a reliable cross-driver representation for custom forms and loaded media.

Some Windows Client queues send an opaque PrintTicket media name even when their PWG Raster page
contains an exact physical size. An opaque job-level media name therefore does not override raster
geometry. ShaPrint maps the first raster page to a named catalog entry within 0.5 mm, including
`om_folio_210x330mm`, or to a bounded legacy custom size. Explicit named media and parseable legacy
custom media retain precedence. Sizes outside the former 50 by 50 mm minimum and 330.2 by 450 mm
maximum are rejected.

Windows caches IPP capabilities in an installed Client queue. The My printers page therefore has an
explicit Repair paper sizes action. After confirmation and UAC approval, repair recreates only the
derived ShaPrint queue when its destination is the expected loopback URI or a Windows `WSD-*` port.
It refuses unrelated queues and queues with pending Print jobs. Recreating a queue can reset its
Windows printing preferences. Normal installation remains idempotent and does not replace a current
working queue.

This decision replaces ADR 0005's requirement that removal always remain manual and ADR 0010's
manual remove-and-install guidance for stale capabilities. Dedicated ports, queue URI paths,
Network Channel authentication, and TLS identity remain unchanged.
