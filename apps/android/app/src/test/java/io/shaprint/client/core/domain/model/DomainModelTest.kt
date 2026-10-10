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
}
