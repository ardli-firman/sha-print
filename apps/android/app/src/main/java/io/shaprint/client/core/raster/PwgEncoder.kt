package io.shaprint.client.core.raster

import io.shaprint.client.core.domain.model.PrintColorMode
import java.io.OutputStream
import java.nio.ByteBuffer

object PwgEncoder {

    /**
     * Builds a 1796-byte PWG 5102.4 page header.
     */
    fun buildHeader(
        width: Int,
        height: Int,
        colorMode: PrintColorMode = PrintColorMode.COLOR,
        dpi: Int = PwgRasterConstants.RESOLUTION_DPI
    ): ByteArray {
        val header = ByteArray(PwgRasterConstants.HEADER_BYTES)
        val colors = if (colorMode == PrintColorMode.COLOR) 3 else 1
        val colorSpace = if (colorMode == PrintColorMode.COLOR) {
            PwgRasterConstants.COLOR_SPACE_SRGB
        } else {
            PwgRasterConstants.COLOR_SPACE_SGRAY
        }

        fun writeBeInt(offset: Int, value: Int) {
            header[offset] = (value ushr 24).toByte()
            header[offset + 1] = (value ushr 16).toByte()
            header[offset + 2] = (value ushr 8).toByte()
            header[offset + 3] = value.toByte()
        }

        writeBeInt(276, dpi) // HWResolutionX
        writeBeInt(280, dpi) // HWResolutionY
        writeBeInt(372, width)
        writeBeInt(376, height)
        writeBeInt(384, PwgRasterConstants.BITS_PER_COLOR)
        writeBeInt(388, colors * PwgRasterConstants.BITS_PER_COLOR)
        writeBeInt(392, width * colors) // BytesPerLine
        writeBeInt(396, 0) // ColorOrder
        writeBeInt(400, colorSpace)
        writeBeInt(420, colors) // NumColors

        return header
    }

    /**
     * Encodes a single raster row using PWG 5102.4 PackBits compression into an OutputStream.
     * @param rowPixels Byte array containing raw pixel bytes (RGB or Grayscale) for the full row width.
     * @param colors 3 for sRGB, 1 for sGray.
     * @param repeats Number of times this row repeats (1 to 256). Stored as repeats - 1 (0 to 255).
     */
    fun encodeRow(
        rowPixels: ByteArray,
        colors: Int,
        out: OutputStream,
        repeats: Int = 1
    ) {
        val safeRepeats = repeats.coerceIn(1, 256)
        out.write(safeRepeats - 1) // repeat count byte

        val totalPixels = rowPixels.size / colors
        var pixelIdx = 0

        while (pixelIdx < totalPixels) {
            // Check for run of identical pixels
            var runLen = 1
            val currentOffset = pixelIdx * colors

            while (pixelIdx + runLen < totalPixels && runLen < 128) {
                val nextOffset = (pixelIdx + runLen) * colors
                if (pixelsEqual(rowPixels, currentOffset, nextOffset, colors)) {
                    runLen++
                } else {
                    break
                }
            }

            if (runLen > 1) {
                // Repeated run: control = runLen - 1 (0..127)
                out.write(runLen - 1)
                out.write(rowPixels, currentOffset, colors)
                pixelIdx += runLen
            } else {
                // Look for literal uncompressed run of distinct pixels
                var litLen = 1
                while (pixelIdx + litLen < totalPixels && litLen < 128) {
                    val p1 = (pixelIdx + litLen - 1) * colors
                    val p2 = (pixelIdx + litLen) * colors
                    // If we encounter identical pixels of length >= 2, stop literal run
                    if (pixelIdx + litLen + 1 < totalPixels) {
                        val p3 = (pixelIdx + litLen + 1) * colors
                        if (pixelsEqual(rowPixels, p2, p3, colors)) {
                            break
                        }
                    }
                    litLen++
                }

                // Literal run: control = 257 - litLen (129..256)
                val control = 257 - litLen
                out.write(control and 0xFF)
                out.write(rowPixels, pixelIdx * colors, litLen * colors)
                pixelIdx += litLen
            }
        }
    }

    private fun pixelsEqual(data: ByteArray, offsetA: Int, offsetB: Int, length: Int): Boolean {
        for (i in 0 until length) {
            if (data[offsetA + i] != data[offsetB + i]) return false
        }
        return true
    }
}
