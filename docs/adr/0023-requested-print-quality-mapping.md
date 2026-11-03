# Requested print quality negotiation and driver mapping

---
status: accepted
extends: ADR 0010, ADR 0017
---

ShaPrint client users printing through native client queues currently see standard driverless
printing options (paper sizes, color/monochrome, duplex, copies, orientation) without quality
selection. Inkjet and laser printers connected to the server support multiple physical quality
modes (draft, normal, high), but clients lack a standard way to request them without vendor-specific
drivers.

Sending higher-resolution raster streams (such as 600 DPI) over the wire would quadruple document
payloads (~30–80 MiB per page), stressing office network bandwidth and exceeding the server's
256 MiB memory bounds (ADR 0010). Furthermore, vendor printer drivers achieve high-quality output
primarily through multi-pass printhead sweeps, fine micro-weave dithering, and mechanical feed
accuracy rather than requiring higher client raster resolution.

ShaPrint now advertises and maps standard IPP print quality attributes across all shared printers:

1. **IPP Attribute Advertisement:**
   The IPPS endpoint advertises `print-quality-supported` with values `[3, 4, 5]` (`draft`, `normal`,
   `high`) and `print-quality-default` with `4` (`normal`). In alignment with ADR 0017, this is
   published as a static global capability across all shared printers to avoid fragile cross-driver
   capability queries.

2. **Raster Resolution Preservation:**
   ShaPrint retains the existing 300 DPI wire raster format (`pwg-raster-document-resolution-supported: 300dpi`).
   The client renders documents at 300 DPI, keeping network transmission lightweight and fast, while
   the physical driver at the server handles mechanical high-density screening.

3. **Server Driver Mapping:**
   - **Windows:** The server maps requested IPP `print-quality` to `DEVMODEW.dmPrintQuality`
     (`DM_PRINTQUALITY = 0x00000400`):
     - `draft` (3) maps to `DMRES_DRAFT` (`-1`).
     - `normal` (4) maps to `DMRES_MEDIUM` (`-3`).
     - `high` (5) maps to `DMRES_HIGH` (`-4`).
     - When the client omits `print-quality` (e.g., Stable clients or basic applications), the
       server leaves `dmPrintQuality` unflagged, preserving the queue's loaded default printing
       preferences.
   - **Linux (CUPS):** The server maps requested quality to the standard CUPS IPP option
     `("print-quality", "3" | "4" | "5")`.

4. **Client Queue Refresh:**
   Because Windows caches IPP capabilities at queue installation time, client queues installed prior
   to this capability update can be refreshed using the existing Repair action on the My printers
   page (ADR 0017) or by reinstalling the queue.

5. **Exclusion of Vendor-Specific Media Types and Input Bins:**
   Proprietary media types (`dmMediaType`) and hardware tray selection (`dmDefaultSource`) remain
   deferred. Single-tray consumer inkjets and multi-tray office copiers use incompatible tray indices
   and proprietary media IDs that risk job rejection when generalized.

## Compatibility

This document introduces no breaking changes. Stable Clients can print to Nightly Servers (unspecified
quality inherits server printing preferences), and Nightly Clients can print to Stable Servers
(unknown attributes are ignored per RFC 8011 §5.2.8).

Dedicated ports (`48631`, `48632`, `48633`), IPPS wire protocol, Network Channel authentication
headers, queue URI paths, and TLS identity storage remain unchanged. Persisted settings, Trusted
Servers, and shared printer selections remain fully backward-compatible if downgraded.
