package io.shaprint.client.features.printservice

import io.shaprint.client.core.domain.model.DuplexMode
import io.shaprint.client.core.domain.model.PrintColorMode
import io.shaprint.client.core.domain.model.PrintJobRequest
import io.shaprint.client.core.domain.model.PrintOrientation
import io.shaprint.client.core.domain.model.RequestedMedia

/**
 * Pure mapping logic between Android System Print framework attributes and ShaPrint domain models.
 */
object PrintServiceMapper {

    /**
     * Maps an Android `PrintAttributes.MediaSize` ID (or custom F4 ID) to ShaPrint `RequestedMedia`.
     */
    fun mapSystemMediaSizeId(mediaId: String?): RequestedMedia {
        return when (mediaId?.uppercase()) {
            "ISO_A4" -> RequestedMedia.ISO_A4
            "NA_LETTER" -> RequestedMedia.NA_LETTER
            "OM_FOLIO", "F4", "FOLIO" -> RequestedMedia.OM_FOLIO
            "NA_LEGAL" -> RequestedMedia.NA_LEGAL
            "ISO_A3" -> RequestedMedia.ISO_A3
            "ISO_A5" -> RequestedMedia.ISO_A5
            else -> RequestedMedia.ISO_A4
        }
    }

    /**
     * Maps ShaPrint `RequestedMedia` to an Android `PrintAttributes.MediaSize` identifier and dimensions in mils (1/1000 inch).
     */
    data class SystemMediaDimensions(
        val id: String,
        val label: String,
        val widthMils: Int,
        val heightMils: Int
    )

    fun toSystemMediaDimensions(media: RequestedMedia): SystemMediaDimensions {
        return when (media) {
            RequestedMedia.ISO_A4 -> SystemMediaDimensions("ISO_A4", media.displayName, 8270, 11690)
            RequestedMedia.NA_LETTER -> SystemMediaDimensions("NA_LETTER", media.displayName, 8500, 11000)
            RequestedMedia.OM_FOLIO -> SystemMediaDimensions("OM_FOLIO", media.displayName, 8270, 13000)
            RequestedMedia.NA_LEGAL -> SystemMediaDimensions("NA_LEGAL", media.displayName, 8500, 14000)
            RequestedMedia.ISO_A3 -> SystemMediaDimensions("ISO_A3", media.displayName, 11690, 16540)
            RequestedMedia.ISO_A5 -> SystemMediaDimensions("ISO_A5", media.displayName, 5830, 8270)
        }
    }

    /**
     * Builds a `PrintJobRequest` from extracted system print parameters.
     */
    fun toPrintJobRequest(
        printerLocalId: String,
        jobLabel: String,
        copies: Int,
        mediaId: String?,
        isColor: Boolean,
        duplexModeCode: Int,
        isPortrait: Boolean
    ): PrintJobRequest? {
        // Local ID format: "<serverCanonicalAddress>/<printerName>"
        val slashIdx = printerLocalId.indexOf('/')
        if (slashIdx <= 0 || slashIdx >= printerLocalId.length - 1) return null

        val serverCanonicalAddress = printerLocalId.substring(0, slashIdx)
        val printerName = printerLocalId.substring(slashIdx + 1)

        val duplex = when (duplexModeCode) {
            2 -> DuplexMode.TWO_SIDED_LONG_EDGE   // PrintAttributes.DUPLEX_MODE_LONG_EDGE
            4 -> DuplexMode.TWO_SIDED_SHORT_EDGE  // PrintAttributes.DUPLEX_MODE_SHORT_EDGE
            else -> DuplexMode.ONE_SIDED
        }

        return PrintJobRequest(
            printerName = printerName,
            serverCanonicalAddress = serverCanonicalAddress,
            jobName = jobLabel.ifBlank { "Android-System-Print" },
            copies = copies.coerceIn(1, 999),
            media = mapSystemMediaSizeId(mediaId),
            colorMode = if (isColor) PrintColorMode.COLOR else PrintColorMode.MONOCHROME,
            duplexMode = duplex,
            orientation = if (isPortrait) PrintOrientation.PORTRAIT else PrintOrientation.LANDSCAPE,
            fitToPage = true
        )
    }

    fun formatPrinterLocalId(serverCanonicalAddress: String, printerName: String): String {
        return "$serverCanonicalAddress/$printerName"
    }
}
