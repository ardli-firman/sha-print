package io.shaprint.client.features.printservice

import io.shaprint.client.core.domain.model.DuplexMode
import io.shaprint.client.core.domain.model.PrintColorMode
import io.shaprint.client.core.domain.model.PrintOrientation
import io.shaprint.client.core.domain.model.RequestedMedia
import org.junit.Assert.*
import org.junit.Test

class PrintServiceMapperTest {

    @Test
    fun mapsSystemMediaIdsIncludingF4Folio() {
        assertEquals(RequestedMedia.ISO_A4, PrintServiceMapper.mapSystemMediaSizeId("ISO_A4"))
        assertEquals(RequestedMedia.OM_FOLIO, PrintServiceMapper.mapSystemMediaSizeId("OM_FOLIO"))
        assertEquals(RequestedMedia.OM_FOLIO, PrintServiceMapper.mapSystemMediaSizeId("F4"))
        assertEquals(RequestedMedia.NA_LETTER, PrintServiceMapper.mapSystemMediaSizeId("NA_LETTER"))
        assertEquals(RequestedMedia.ISO_A4, PrintServiceMapper.mapSystemMediaSizeId("UNKNOWN_SIZE"))
    }

    @Test
    fun convertsRequestedMediaToSystemMilsDimensions() {
        val f4Dims = PrintServiceMapper.toSystemMediaDimensions(RequestedMedia.OM_FOLIO)
        assertEquals("OM_FOLIO", f4Dims.id)
        assertEquals(8270, f4Dims.widthMils)
        assertEquals(13000, f4Dims.heightMils)
    }

    @Test
    fun buildsPrintJobRequestFromSystemPrintParameters() {
        val localId = PrintServiceMapper.formatPrinterLocalId("192.168.1.100:48631", "Epson L3210")
        assertEquals("192.168.1.100:48631/Epson L3210", localId)

        val request = PrintServiceMapper.toPrintJobRequest(
            printerLocalId = localId,
            jobLabel = "Gmail Email Print",
            copies = 3,
            mediaId = "OM_FOLIO",
            isColor = false,
            duplexModeCode = 2, // Long Edge
            isPortrait = false
        )

        assertNotNull(request)
        assertEquals("Epson L3210", request?.printerName)
        assertEquals("192.168.1.100:48631", request?.serverCanonicalAddress)
        assertEquals("Gmail Email Print", request?.jobName)
        assertEquals(3, request?.copies)
        assertEquals(RequestedMedia.OM_FOLIO, request?.media)
        assertEquals(PrintColorMode.MONOCHROME, request?.colorMode)
        assertEquals(DuplexMode.TWO_SIDED_LONG_EDGE, request?.duplexMode)
        assertEquals(PrintOrientation.LANDSCAPE, request?.orientation)
    }

    @Test
    fun rejectsMalformedPrinterLocalId() {
        assertNull(PrintServiceMapper.toPrintJobRequest("invalid-no-slash", "Job", 1, "ISO_A4", true, 1, true))
        assertNull(PrintServiceMapper.toPrintJobRequest("/missing-server", "Job", 1, "ISO_A4", true, 1, true))
        assertNull(PrintServiceMapper.toPrintJobRequest("192.168.1.1:48631/", "Job", 1, "ISO_A4", true, 1, true))
    }
}
