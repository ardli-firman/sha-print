package io.shaprint.client.core.domain.model

import org.junit.Assert.assertEquals
import org.junit.Assert.assertNotNull
import org.junit.Test

class DomainModelTest {

    @Test
    fun canonicalAddressFormatsHostAndPort() {
        val nearby = NearbyServer(
            host = "192.168.1.50",
            port = 48631,
            computerName = "DESKTOP-PRINT"
        )
        assertEquals("192.168.1.50:48631", nearby.canonicalAddress)

        val trusted = TrustedServer(
            host = "print.lan",
            port = 48631,
            computerName = "SERVER",
            sha256Fingerprint = "AA:BB:CC"
        )
        assertEquals("print.lan:48631", trusted.canonicalAddress)
    }

    @Test
    fun requestedMediaMapsIppNamesCorrectly() {
        assertEquals(RequestedMedia.ISO_A4, RequestedMedia.fromIppName("iso_a4_210x297mm"))
        assertEquals(RequestedMedia.OM_FOLIO, RequestedMedia.fromIppName("om_folio_210x330mm"))
        assertEquals(RequestedMedia.NA_LETTER, RequestedMedia.fromIppName("na_letter_8.5x11in"))
        assertNotNull(RequestedMedia.fromIppName("na_legal_8.5x14in"))
    }

    @Test
    fun percentEncodesPrinterNameInPrintJobRequestUri() {
        val requestWithSpaces = PrintJobRequest(
            printerName = "HP LaserJet Pro MFP",
            serverCanonicalAddress = "192.168.1.100:48631"
        )
        assertEquals("HP%20LaserJet%20Pro%20MFP", requestWithSpaces.encodedPrinterName)
        assertEquals(
            "ipp://192.168.1.100:48631/ipp/print/HP%20LaserJet%20Pro%20MFP",
            requestWithSpaces.printerIppUri
        )

        val requestWithSymbols = PrintJobRequest(
            printerName = "Queue#1/2 (Office)",
            serverCanonicalAddress = "192.168.1.100:48631"
        )
        assertEquals("Queue%231%2F2%20%28Office%29", requestWithSymbols.encodedPrinterName)
    }
}
