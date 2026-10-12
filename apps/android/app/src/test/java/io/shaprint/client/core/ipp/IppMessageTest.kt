package io.shaprint.client.core.ipp

import io.shaprint.client.core.domain.model.DuplexMode
import io.shaprint.client.core.domain.model.PrintColorMode
import io.shaprint.client.core.domain.model.RequestedMedia
import org.junit.Assert.*
import org.junit.Test
import java.io.ByteArrayOutputStream
import java.io.DataOutputStream

class IppMessageTest {

    @Test
    fun buildsValidGetPrintersRequestWithNetworkChannel() {
        val bytes = IppMessage.buildGetPrintersRequest(
            printerUri = "ipp://192.168.1.50:48631/ipp/print",
            networkChannel = "secret-channel-xyz",
            requestId = 42
        )

        assertTrue(bytes.size > 20)
        // Version 2.0 (0x0200)
        assertEquals(0x02, bytes[0].toInt())
        assertEquals(0x00, bytes[1].toInt())
        // Operation ID OP_CUPS_GET_PRINTERS (0x4002)
        assertEquals(0x40, bytes[2].toInt())
        assertEquals(0x02, bytes[3].toInt())
        // Request ID = 42
        assertEquals(42, bytes[7].toInt())

        // Group tag: 0x01 (Operation Attributes)
        assertEquals(0x01, bytes[8].toInt())

        // Last byte: 0x03 (End of attributes)
        assertEquals(0x03, bytes[bytes.size - 1].toInt())

        val asString = String(bytes, Charsets.UTF_8)
        assertTrue(asString.contains("secret-channel-xyz"))
        assertTrue(asString.contains("shaprint-android"))
        assertTrue(asString.contains("attributes-charset"))
    }

    @Test
    fun buildsValidGetPrinterAttributesRequest() {
        val bytes = IppMessage.buildGetPrintersRequest(
            printerUri = "ipps://192.168.1.50:48631/ipp/print/OfficeLaser",
            operationId = IppConstants.OP_GET_PRINTER_ATTRIBUTES,
            requestId = 99
        )

        assertEquals(0x02, bytes[0].toInt())
        assertEquals(0x00, bytes[1].toInt())
        // Operation ID OP_GET_PRINTER_ATTRIBUTES (0x000B)
        assertEquals(0x00, bytes[2].toInt())
        assertEquals(0x0B, bytes[3].toInt())
        assertEquals(99, bytes[7].toInt())
    }

    @Test
    fun parsesMultiPrinterIppResponseWithCapabilities() {
        val baos = ByteArrayOutputStream()
        val dos = DataOutputStream(baos)

        // Header: Version 0x0200, Status OK (0x0000), Request ID 1
        dos.writeShort(0x0200)
        dos.writeShort(0x0000)
        dos.writeInt(1)

        // Operation attributes group
        dos.writeByte(0x01)
        writeAttr(dos, 0x47, "attributes-charset", "utf-8")
        writeAttr(dos, 0x48, "attributes-natural-language", "en")

        // Printer 1: Epson L3210
        dos.writeByte(0x04) // Printer attributes group
        writeAttr(dos, 0x42, "printer-name", "Epson L3210 Series")
        writeAttr(dos, 0x45, "printer-uri-supported", "ipp://192.168.1.100:48631/ipp/print/Epson-L3210")
        writeAttr(dos, 0x44, "media-supported", "iso_a4_210x297mm")
        writeAttr(dos, 0x44, "", "om_folio_210x330mm") // 1setOf
        writeAttr(dos, 0x44, "", "na_letter_8.5x11in")
        writeAttr(dos, 0x44, "print-color-mode-supported", "color")
        writeAttr(dos, 0x44, "", "monochrome")
        writeAttr(dos, 0x44, "sides-supported", "one-sided")
        writeAttr(dos, 0x44, "", "two-sided-long-edge")
        // copies-supported range: 1 to 999 (8 bytes)
        dos.writeByte(0x33)
        val copiesName = "copies-supported".toByteArray(Charsets.UTF_8)
        dos.writeShort(copiesName.size)
        dos.write(copiesName)
        dos.writeShort(8)
        dos.writeInt(1)
        dos.writeInt(999)

        // Printer 2: HP LaserJet
        dos.writeByte(0x04) // Printer attributes group
        writeAttr(dos, 0x42, "printer-name", "HP LaserJet Pro")
        writeAttr(dos, 0x45, "printer-uri-supported", "ipp://192.168.1.100:48631/ipp/print/HP-LaserJet")
        writeAttr(dos, 0x44, "media-supported", "iso_a4_210x297mm")
        writeAttr(dos, 0x44, "print-color-mode-supported", "monochrome")
        writeAttr(dos, 0x44, "sides-supported", "one-sided")

        // End tag
        dos.writeByte(0x03)

        val responseBytes = baos.toByteArray()
        val parsed = IppMessage.parseResponse(responseBytes, "192.168.1.100:48631")

        assertEquals(0x0000.toShort(), parsed.statusCode)
        assertEquals(1, parsed.requestId)
        assertEquals(2, parsed.printers.size)

        val epson = parsed.printers[0]
        assertEquals("Epson L3210 Series", epson.name)
        assertEquals("192.168.1.100:48631", epson.serverCanonicalAddress)
        assertTrue(epson.capabilities.supportedMedia.contains(RequestedMedia.ISO_A4))
        assertTrue(epson.capabilities.supportedMedia.contains(RequestedMedia.OM_FOLIO))
        assertTrue(epson.capabilities.supportedMedia.contains(RequestedMedia.NA_LETTER))
        assertTrue(epson.capabilities.supportedColorModes.contains(PrintColorMode.COLOR))
        assertTrue(epson.capabilities.supportedColorModes.contains(PrintColorMode.MONOCHROME))
        assertTrue(epson.capabilities.supportedDuplexModes.contains(DuplexMode.TWO_SIDED_LONG_EDGE))
        assertEquals(999, epson.capabilities.maxCopies)

        val hp = parsed.printers[1]
        assertEquals("HP LaserJet Pro", hp.name)
        assertTrue(hp.capabilities.supportedColorModes.contains(PrintColorMode.MONOCHROME))
        assertFalse(hp.capabilities.supportedColorModes.contains(PrintColorMode.COLOR))
    }

    @Test
    fun stopsParsingOnTruncatedAttributeNameInsteadOfMisinterpretingAs1SetOf() {
        val baos = ByteArrayOutputStream()
        val dos = DataOutputStream(baos)

        dos.writeShort(0x0200)
        dos.writeShort(0x0000)
        dos.writeInt(1)
        dos.writeByte(0x04) // Printer attributes group
        writeAttr(dos, 0x42, "printer-name", "Valid-Printer")

        // Write truncated attribute with nameLen = 50, but only 3 bytes remaining
        dos.writeByte(0x44)
        dos.writeShort(50)
        dos.writeBytes("abc")

        val parsed = IppMessage.parseResponse(baos.toByteArray(), "192.168.1.100:48631")
        assertEquals(0, parsed.printers.size)
    }

    @Test
    fun buildsValidPrintJobHeaderWithIppsSchemeAndPwgMimeType() {
        val request = io.shaprint.client.core.domain.model.PrintJobRequest(
            printerName = "Epson L3210 Series",
            serverCanonicalAddress = "192.168.1.50:48631",
            jobName = "Invoice_2026.pdf",
            copies = 2,
            media = RequestedMedia.ISO_A4,
            colorMode = PrintColorMode.COLOR,
            duplexMode = DuplexMode.TWO_SIDED_LONG_EDGE
        )

        val bytes = IppMessage.buildPrintJobHeader(
            jobRequest = request,
            networkChannel = "secret-network-key",
            requestId = 77
        )

        // Version 2.0 (0x0200)
        assertEquals(0x02, bytes[0].toInt())
        assertEquals(0x00, bytes[1].toInt())
        // Operation ID OP_PRINT_JOB (0x0002)
        assertEquals(0x00, bytes[2].toInt())
        assertEquals(0x02, bytes[3].toInt())
        // Request ID = 77
        assertEquals(77, bytes[7].toInt())

        // Operation attributes tag: 0x01
        assertEquals(0x01, bytes[8].toInt())

        // Last byte before payload: TAG_END_OF_ATTRIBUTES (0x03)
        assertEquals(0x03, bytes[bytes.size - 1].toInt())

        val asString = String(bytes, Charsets.UTF_8)
        // printer-uri MUST use ipps:// scheme matching server find_by_uri requirement
        assertTrue(asString.contains("ipps://192.168.1.50:48631/ipp/print/Epson%20L3210%20Series"))
        assertTrue(asString.contains("image/pwg-raster"))
        assertTrue(asString.contains("secret-network-key"))
        assertTrue(asString.contains("Invoice_2026.pdf"))
    }

    @Test
    fun parsesJobResponseStatus() {
        val okBytes = byteArrayOf(0x02, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x01, 0x03)
        val okResult = IppMessage.parseJobResponse(okBytes)
        assertTrue(okResult.isSuccess)
        assertEquals(IppConstants.STATUS_OK, okResult.statusCode)

        val unauthBytes = byteArrayOf(0x02, 0x00, 0x04, 0x03, 0x00, 0x00, 0x00, 0x01, 0x03)
        val unauthResult = IppMessage.parseJobResponse(unauthBytes)
        assertFalse(unauthResult.isSuccess)
        assertEquals(IppConstants.STATUS_CLIENT_ERROR_NOT_AUTHORIZED, unauthResult.statusCode)

        val forbiddenBytes = byteArrayOf(0x02, 0x00, 0x04, 0x01, 0x00, 0x00, 0x00, 0x01, 0x03)
        val forbiddenResult = IppMessage.parseJobResponse(forbiddenBytes)
        assertFalse(forbiddenResult.isSuccess)
        assertEquals(IppConstants.STATUS_CLIENT_ERROR_FORBIDDEN, forbiddenResult.statusCode)

        val badRequestBytes = byteArrayOf(0x02, 0x00, 0x04, 0x00, 0x00, 0x00, 0x00, 0x01, 0x03)
        val badRequestResult = IppMessage.parseJobResponse(badRequestBytes)
        assertFalse(badRequestResult.isSuccess)
        assertEquals(IppConstants.STATUS_CLIENT_ERROR_BAD_REQUEST, badRequestResult.statusCode)
        assertTrue(badRequestResult.errorMessage?.contains("0x0400") == true)

        val internalErrorBytes = byteArrayOf(0x02, 0x00, 0x05, 0x00, 0x00, 0x00, 0x00, 0x01, 0x03)
        val internalErrorResult = IppMessage.parseJobResponse(internalErrorBytes)
        assertFalse(internalErrorResult.isSuccess)
        assertEquals(IppConstants.STATUS_SERVER_ERROR_INTERNAL, internalErrorResult.statusCode)
        assertTrue(internalErrorResult.errorMessage?.contains("Windows printer queue") == true)
        assertTrue(internalErrorResult.errorMessage?.contains("0x0500") == true)

        val notAcceptingBytes = byteArrayOf(0x02, 0x00, 0x05, 0x04, 0x00, 0x00, 0x00, 0x01, 0x03)
        val notAcceptingResult = IppMessage.parseJobResponse(notAcceptingBytes)
        assertFalse(notAcceptingResult.isSuccess)
        assertEquals(IppConstants.STATUS_SERVER_ERROR_NOT_ACCEPTING_JOBS, notAcceptingResult.statusCode)
        assertTrue(notAcceptingResult.errorMessage?.contains("not accepting jobs") == true)
        assertTrue(notAcceptingResult.errorMessage?.contains("0x0504") == true)
    }

    private fun writeAttr(dos: DataOutputStream, tag: Byte, name: String, value: String) {
        dos.writeByte(tag.toInt())
        val nameBytes = name.toByteArray(Charsets.UTF_8)
        dos.writeShort(nameBytes.size)
        dos.write(nameBytes)

        val valBytes = value.toByteArray(Charsets.UTF_8)
        dos.writeShort(valBytes.size)
        dos.write(valBytes)
    }
}
