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
    val printerIppUri: String
        get() = "ipp://$serverCanonicalAddress/ipp/print/$printerName"
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
