package io.shaprint.client.core.raster

import io.shaprint.client.core.domain.model.PrintColorMode
import org.junit.Assert.*
import org.junit.Test
import java.io.ByteArrayOutputStream
import java.nio.ByteBuffer

class PwgEncoderTest {

    @Test
    fun buildsValid1796BytePwgHeaderMatchingServerExpectations() {
        val header = PwgEncoder.buildHeader(
            width = 2480,
            height = 3508,
            colorMode = PrintColorMode.COLOR,
            dpi = 300
        )

        assertEquals(1796, header.size)

        fun readBeInt(offset: Int): Int {
            return ByteBuffer.wrap(header, offset, 4).int
        }

        // Exact offsets required by apps/desktop/src-tauri/src/adapters/printers/raster.rs
        assertEquals(300, readBeInt(276))       // HWResolutionX
        assertEquals(300, readBeInt(280))       // HWResolutionY
        assertEquals(2480, readBeInt(372))      // Width
        assertEquals(3508, readBeInt(376))      // Height
        assertEquals(8, readBeInt(384))         // BitsPerColor
        assertEquals(24, readBeInt(388))        // BitsPerPixel (3 * 8)
        assertEquals(2480 * 3, readBeInt(392))  // BytesPerLine
        assertEquals(0, readBeInt(396))         // ColorOrder
        assertEquals(19, readBeInt(400))        // ColorSpace (19 = sRGB)
        assertEquals(3, readBeInt(420))         // NumColors
    }

    @Test
    fun buildsValidMonochromePwgHeader() {
        val header = PwgEncoder.buildHeader(
            width = 100,
            height = 200,
            colorMode = PrintColorMode.MONOCHROME,
            dpi = 300
        )

        fun readBeInt(offset: Int): Int = ByteBuffer.wrap(header, offset, 4).int

        assertEquals(8, readBeInt(384))
        assertEquals(8, readBeInt(388))         // 1 * 8
        assertEquals(100, readBeInt(392))       // 100 * 1
        assertEquals(18, readBeInt(400))        // 18 = sGray
        assertEquals(1, readBeInt(420))         // 1 color
    }

    @Test
    fun packBitsRowEncodingRoundTripsThroughServerDecoderAlgorithm() {
        val width = 8
        val colors = 3
        // 4 white pixels (FF FF FF), 2 red pixels (FF 00 00), 1 green (00 FF 00), 1 blue (00 00 FF)
        val rowPixels = byteArrayOf(
            0xFF.toByte(), 0xFF.toByte(), 0xFF.toByte(),
            0xFF.toByte(), 0xFF.toByte(), 0xFF.toByte(),
            0xFF.toByte(), 0xFF.toByte(), 0xFF.toByte(),
            0xFF.toByte(), 0xFF.toByte(), 0xFF.toByte(),
            0xFF.toByte(), 0x00.toByte(), 0x00.toByte(),
            0xFF.toByte(), 0x00.toByte(), 0x00.toByte(),
            0x00.toByte(), 0xFF.toByte(), 0x00.toByte(),
            0x00.toByte(), 0x00.toByte(), 0xFF.toByte()
        )

        val baos = ByteArrayOutputStream()
        PwgEncoder.encodeRow(rowPixels, colors, baos, repeats = 1)
        val encoded = baos.toByteArray()

        // Decode using exact algorithm from apps/desktop/src-tauri/src/adapters/printers/raster.rs
        var pos = 0
        val repeats = (encoded[pos++].toInt() and 0xFF) + 1
        assertEquals(1, repeats)

        val decodedRow = ByteArray(width * colors)
        var x = 0
        while (x < width) {
            val control = encoded[pos++].toInt() and 0xFF
            val count = if (control < 128) {
                control + 1
            } else {
                assertNotEquals(128, control)
                257 - control
            }

            if (control < 128) {
                // Repeated pixel
                val pixelBytes = encoded.copyOfRange(pos, pos + colors)
                pos += colors
                for (p in 0 until count) {
                    System.arraycopy(pixelBytes, 0, decodedRow, (x + p) * colors, colors)
                }
            } else {
                // Literal pixels
                val byteCount = count * colors
                System.arraycopy(encoded, pos, decodedRow, x * colors, byteCount)
                pos += byteCount
            }
            x += count
        }

        assertEquals(encoded.size, pos)
        assertArrayEquals(rowPixels, decodedRow)
    }

    @Test
    fun mmToPixelsCalculatesStandard300DpiDimensions() {
        // A4: 210 x 297 mm -> 2480 x 3508 px
        assertEquals(2480, ImageRasterizer.mmToPixels(210f, 300))
        assertEquals(3508, ImageRasterizer.mmToPixels(297f, 300))

        // F4 / Folio: 210 x 330 mm -> 2480 x 3898 px
        assertEquals(2480, ImageRasterizer.mmToPixels(210f, 300))
        assertEquals(3898, ImageRasterizer.mmToPixels(330f, 300))
    }
}
