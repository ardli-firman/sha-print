package io.shaprint.client.core.raster

/**
 * PWG 5102.4 Raster format constants (ADR 0010).
 */
object PwgRasterConstants {
    /** 4-byte synchronization word at the start of a PWG-raster stream: 'RaS2' (ADR 0010) */
    val SYNC_WORD = byteArrayOf('R'.code.toByte(), 'a'.code.toByte(), 'S'.code.toByte(), '2'.code.toByte())

    /** Size of a PWG-raster page header in bytes per PWG 5102.4 */
    const val HEADER_BYTES = 1796

    /** Document format MIME type accepted by ShaPrint server */
    const val MIME_TYPE_PWG_RASTER = "image/pwg-raster"

    /** Standard resolution for ShaPrint client rasterization */
    const val RESOLUTION_DPI = 300

    /** Color space: sRGB */
    const val COLOR_SPACE_SRGB: Int = 19

    /** Color space: sGray */
    const val COLOR_SPACE_SGRAY: Int = 18

    /** Bits per color: 8 bits */
    const val BITS_PER_COLOR = 8

    /** Pixel bytes for sRGB (24-bit TrueColor) */
    const val BYTES_PER_PIXEL_SRGB = 3

    /** Pixel bytes for sGray (8-bit Grayscale) */
    const val BYTES_PER_PIXEL_SGRAY = 1
}
