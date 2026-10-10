package io.shaprint.client.core.raster

import android.graphics.Bitmap
import android.graphics.Canvas
import android.graphics.Color
import android.graphics.pdf.PdfRenderer
import io.shaprint.client.core.domain.model.PrintColorMode
import io.shaprint.client.core.domain.model.RequestedMedia
import java.io.OutputStream
import kotlin.math.roundToInt

object PdfRasterizer {

    /**
     * Streams a multi-page PDF document page-by-page directly to a PWG 5102.4 raster output stream.
     * Keeps memory bounded to a single decoded page (~35 MiB) by immediately recycling each page.
     */
    fun streamPdfToPwg(
        pdfRenderer: PdfRenderer,
        media: RequestedMedia,
        colorMode: PrintColorMode,
        out: OutputStream,
        onPageRendered: ((page: Int, total: Int) -> Unit)? = null
    ) {
        val pageCount = pdfRenderer.pageCount
        if (pageCount == 0) return

        // Write the 4-byte stream synchronization header: 'RaS2'
        out.write(PwgRasterConstants.SYNC_WORD)

        val widthPx = ImageRasterizer.mmToPixels(media.widthMm)
        val heightPx = ImageRasterizer.mmToPixels(media.heightMm)
        val colors = if (colorMode == PrintColorMode.COLOR) 3 else 1

        val rowPixelsInts = IntArray(widthPx)
        val rowPixelsBytes = ByteArray(widthPx * colors)

        for (pageIndex in 0 until pageCount) {
            onPageRendered?.invoke(pageIndex + 1, pageCount)

            val page = pdfRenderer.openPage(pageIndex)
            try {
                // Write 1796-byte PWG header for this page
                val header = PwgEncoder.buildHeader(widthPx, heightPx, colorMode)
                out.write(header)

                // Render page to single-page temporary bitmap
                val pageBitmap = Bitmap.createBitmap(widthPx, heightPx, Bitmap.Config.ARGB_8888)
                val canvas = Canvas(pageBitmap)
                canvas.drawColor(Color.WHITE)

                page.render(pageBitmap, null, null, PdfRenderer.Page.RENDER_MODE_FOR_PRINT)

                // Stream rows with PackBits compression
                for (y in 0 until heightPx) {
                    pageBitmap.getPixels(rowPixelsInts, 0, widthPx, 0, y, widthPx, 1)

                    if (colorMode == PrintColorMode.COLOR) {
                        for (x in 0 until widthPx) {
                            val pixel = rowPixelsInts[x]
                            rowPixelsBytes[x * 3] = ((pixel shr 16) and 0xFF).toByte()
                            rowPixelsBytes[x * 3 + 1] = ((pixel shr 8) and 0xFF).toByte()
                            rowPixelsBytes[x * 3 + 2] = (pixel and 0xFF).toByte()
                        }
                    } else {
                        for (x in 0 until widthPx) {
                            val pixel = rowPixelsInts[x]
                            val r = (pixel shr 16) and 0xFF
                            val g = (pixel shr 8) and 0xFF
                            val b = pixel and 0xFF
                            val gray = (0.299 * r + 0.587 * g + 0.114 * b).roundToInt().coerceIn(0, 255)
                            rowPixelsBytes[x] = gray.toByte()
                        }
                    }

                    PwgEncoder.encodeRow(rowPixelsBytes, colors, out)
                }

                pageBitmap.recycle()
            } finally {
                page.close()
            }
        }
    }
}
