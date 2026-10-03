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

The server advertises and accepts `iso_a4_210x297mm`, `na_letter_8.5x11in`, `na_legal_8.5x14in`,
`iso_a3_297x420mm`, `iso_a5_148x210mm`, `om_folio_210x330mm` (210×330 mm F4), and
`na_foolscap_8.5x13in` (8.5×13 in / 215.9×330.2 mm Folio). When the client omits job-level media
or orientation, the first raster page supplies the known standard size and orientation from its
physical dimensions.

When a shared printer's queue already has loaded media (either a custom form or a standard paper
size) whose short and long dimensions are at least as large as the requested media (within a 0.5 mm
rounding tolerance), the adapter preserves the queue's loaded media in `DEVMODE` rather than
overwriting it with the smaller requested media. When a physical driver (such as an Epson L-series
inkjet loaded with F4 paper) is told the paper is A4 (`210×297 mm`), its Portrait feed stops at
297 mm and ejects the remaining ~33 mm at the bottom, whereas in Landscape its coordinate origin is
anchored to the 297 mm width along the feed axis, leaving the ~33 mm blank margin at the trailing
edge (the left side / start of the Landscape document) instead of the leading edge (the right side /
end of the Landscape document). Preserving the larger loaded media keeps the driver's physical sheet
coordinate system on the actual paper while `StretchDIBits` draws the requested page at 1:1 physical
scale at the top-left origin (`-PHYSICALOFFSETX`, `-PHYSICALOFFSETY`). When the requested media does
not fit within the queue's loaded media, or when the queue has no loaded media dimensions, the
adapter selects the requested media (`DMPAPER_*` or explicit `dmPaperLength`/`dmPaperWidth` tenths of
a millimeter for `om_folio_210x330mm`) and clears any smaller custom form. The driver merges the
updated public `DEVMODE` back into its private settings with
`DocumentPropertiesW(DM_IN_BUFFER | DM_OUT_BUFFER)` before creating the drawing context. Application
margins remain part of the raster document.

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
[Windows RAW submission](https://learn.microsoft.com/en-us/windows/win32/printdocs/sending-data-directly-to-a-printer),
[driver setting updates](https://learn.microsoft.com/en-us/troubleshoot/windows/win32/modify-printer-settings-documentproperties).
