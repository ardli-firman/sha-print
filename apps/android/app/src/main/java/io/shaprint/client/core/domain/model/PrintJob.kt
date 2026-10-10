package io.shaprint.client.core.domain.model

/**
 * A document and its requested print settings submitted to a shared printer (see CONTEXT.md).
 */
data class PrintJobRequest(
    val printerName: String,
    val serverCanonicalAddress: String,
    val media: RequestedMedia = RequestedMedia.ISO_A4,
    val colorMode: PrintColorMode = PrintColorMode.COLOR,
    val duplexMode: DuplexMode = DuplexMode.ONE_SIDED,
    val copies: Int = 1,
    val fitToPage: Boolean = true
)

data class PrintJobProgress(
    val currentPage: Int = 0,
    val totalPages: Int = 0,
    val percentTransferred: Int = 0,
    val statusMessage: String = "Idle",
    val isCompleted: Boolean = false,
    val isFailed: Boolean = false,
    val errorMessage: String? = null
)
