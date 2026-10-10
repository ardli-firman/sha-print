package io.shaprint.client.core.domain.model

/**
 * A printer made available by a server to ShaPrint clients (see CONTEXT.md).
 */
data class SharedPrinter(
    val name: String,
    val serverCanonicalAddress: String,
    val isOnline: Boolean = true,
    val capabilities: PrinterCapabilities = PrinterCapabilities()
)

/**
 * Standard paper sizes supported by ShaPrint (ADR 0010).
 */
enum class RequestedMedia(val ippName: String, val displayName: String, val widthMm: Float, val heightMm: Float) {
    ISO_A4("iso_a4_210x297mm", "A4 (210 × 297 mm)", 210f, 297f),
    NA_LETTER("na_letter_8.5x11in", "Letter (8.5 × 11 in)", 215.9f, 279.4f),
    OM_FOLIO("om_folio_210x330mm", "F4 / Folio (210 × 330 mm)", 210f, 330f),
    NA_LEGAL("na_legal_8.5x14in", "Legal (8.5 × 14 in)", 215.9f, 355.6f),
    ISO_A3("iso_a3_297x420mm", "A3 (297 × 420 mm)", 297f, 420f),
    ISO_A5("iso_a5_148x210mm", "A5 (148 × 210 mm)", 148f, 210f);

    companion object {
        fun fromIppName(name: String): RequestedMedia? = entries.firstOrNull { it.ippName.equals(name, ignoreCase = true) }
    }
}

enum class PrintColorMode(val ippName: String, val displayName: String) {
    COLOR("color", "Color"),
    MONOCHROME("monochrome", "Monochrome")
}

enum class DuplexMode(val ippName: String, val displayName: String) {
    ONE_SIDED("one-sided", "Single-sided"),
    TWO_SIDED_LONG_EDGE("two-sided-long-edge", "Double-sided (Long Edge)"),
    TWO_SIDED_SHORT_EDGE("two-sided-short-edge", "Double-sided (Short Edge)")
}

data class PrinterCapabilities(
    val supportedMedia: List<RequestedMedia> = listOf(RequestedMedia.ISO_A4, RequestedMedia.NA_LETTER, RequestedMedia.OM_FOLIO),
    val supportedColorModes: List<PrintColorMode> = listOf(PrintColorMode.COLOR, PrintColorMode.MONOCHROME),
    val supportedDuplexModes: List<DuplexMode> = listOf(DuplexMode.ONE_SIDED),
    val maxCopies: Int = 99
)
