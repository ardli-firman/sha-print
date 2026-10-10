package io.shaprint.client.core.domain.model

enum class PrintOrientation(val ippEnum: Int, val displayName: String) {
    PORTRAIT(3, "Portrait"),
    LANDSCAPE(4, "Landscape")
}

enum class PrintJobState {
    IDLE,
    RASTERIZING,
    STREAMING,
    COMPLETED,
    FAILED
}

/**
 * A document and its requested print settings submitted to a shared printer (see CONTEXT.md).
 */
data class PrintJobRequest(
    val printerName: String,
    val serverCanonicalAddress: String,
    val jobName: String = "ShaPrint-Job",
    val media: RequestedMedia = RequestedMedia.ISO_A4,
    val colorMode: PrintColorMode = PrintColorMode.COLOR,
    val duplexMode: DuplexMode = DuplexMode.ONE_SIDED,
    val orientation: PrintOrientation = PrintOrientation.PORTRAIT,
    val copies: Int = 1,
    val fitToPage: Boolean = true
) {
    val encodedPrinterName: String
        get() = percentEncodeUriSegment(printerName)

    val printerIppUri: String
        get() = "ipp://$serverCanonicalAddress/ipp/print/$encodedPrinterName"

    companion object {
        /**
         * Percent-encodes a printer name segment per RFC 3986 matching the Rust server's `percent_encode`.
         */
        fun percentEncodeUriSegment(value: String): String {
            val sb = StringBuilder(value.length)
            for (byte in value.toByteArray(Charsets.UTF_8)) {
                val b = byte.toInt() and 0xFF
                val isUnreserved = (b in 'a'.code..'z'.code) ||
                    (b in 'A'.code..'Z'.code) ||
                    (b in '0'.code..'9'.code) ||
                    b == '-'.code || b == '.'.code || b == '_'.code || b == '~'.code
                if (isUnreserved) {
                    sb.append(b.toChar())
                } else {
                    sb.append(String.format("%%%02X", b))
                }
            }
            return sb.toString()
        }
    }
}

data class PrintJobProgress(
    val state: PrintJobState = PrintJobState.IDLE,
    val currentPage: Int = 0,
    val totalPages: Int = 0,
    val percentTransferred: Int = 0,
    val statusMessage: String = "Idle",
    val isCompleted: Boolean = state == PrintJobState.COMPLETED,
    val isFailed: Boolean = state == PrintJobState.FAILED,
    val errorMessage: String? = null
)
