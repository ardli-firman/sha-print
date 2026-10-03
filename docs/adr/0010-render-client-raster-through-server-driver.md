# Render client raster pages through the server printer driver

---
status: accepted
---

A native client queue uses Microsoft IPP Class Driver, which produces a negotiated document
format rather than the server printer's device commands. Sending PWG Raster, PDF, OXPS, or PCLm
to `WritePrinter` as RAW bypasses the server's installed driver. A USB inkjet such as Epson L3210
can then print raster headers and encoded data as text even though its original local queue prints
normally. Queue acceptance alone does not establish correct visual output.

The server now advertises and accepts only `image/pwg-raster`, with `srgb_8` and `sgray_8` at
300 dpi and normal sheet backs. It decodes the PWG 5102.4 page headers and compressed rows into
top-down BGRX bitmaps, then uses GDI `CreateDCW`, `StartDocW`, and `StretchDIBits` through the
selected shared printer's installed driver. Physical page size comes from raster dimensions and
resolution; the printer's unprintable margins clip the sheet rather than shrinking its contents.
Common job settings continue to use the queue's DEVMODE. This replaces the RAW submission and
document-format portions of ADR 0003. It does not change IPPS authorization or certificate approval.

PDF, OXPS, PCLm, and opaque octet streams are rejected before submission because they have no
renderer in this application. Applications can still print their documents through the native
client queue, where Windows renders them into PWG Raster. Supporting another wire format requires
a renderer and tests before adding it to `document-format-supported`.

Decoding validates the complete document before opening a printer job, rejects unsupported pixel
layouts and malformed row runs, and limits each decoded page to 256 MiB and each job to 1000 pages.
The adapter validates the full job before opening the queue, then decodes it again one page at a
time while rendering, so longer documents do not retain every bitmap in memory. Pages
are limited to 20,000 pixels per dimension. A failed GDI operation aborts the job. Tests use generated
color samples, never captured user documents. A Windows smoke test renders a black square through
the real adapter, and visual comparison of a Windows Test Page through the native client queue
remains required on a physical printer. Existing client queues may cache old capabilities and need
manual removal and installation again after the server update; installation still never deletes
an existing queue.

References: [PWG Raster specification](https://ftp.pwg.org/pub/pwg/candidates/cs-ippraster10-20120420-5102.4.pdf),
[Windows RAW submission](https://learn.microsoft.com/en-us/windows/win32/printdocs/sending-data-directly-to-a-printer).
