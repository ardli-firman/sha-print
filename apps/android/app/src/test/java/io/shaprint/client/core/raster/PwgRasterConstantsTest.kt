package io.shaprint.client.core.raster

import org.junit.Assert.assertArrayEquals
import org.junit.Assert.assertEquals
import org.junit.Test

class PwgRasterConstantsTest {

    @Test
    fun pwgConstantsAdhereToAdr0010() {
        assertEquals("image/pwg-raster", PwgRasterConstants.MIME_TYPE_PWG_RASTER)
        assertEquals(300, PwgRasterConstants.RESOLUTION_DPI)
        assertEquals(19, PwgRasterConstants.COLOR_SPACE_SRGB)
        assertEquals(18, PwgRasterConstants.COLOR_SPACE_SGRAY)
        assertEquals(8, PwgRasterConstants.BITS_PER_COLOR)
        assertEquals(3, PwgRasterConstants.BYTES_PER_PIXEL_SRGB)
        assertEquals(1, PwgRasterConstants.BYTES_PER_PIXEL_SGRAY)

        val expectedSync = byteArrayOf('R'.code.toByte(), 'a'.code.toByte(), 'S'.code.toByte(), 't'.code.toByte())
        assertArrayEquals(expectedSync, PwgRasterConstants.SYNC_WORD)
    }
}
