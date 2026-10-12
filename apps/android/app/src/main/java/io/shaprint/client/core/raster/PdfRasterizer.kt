package io.shaprint.client.core.raster

import android.graphics.Bitmap
import android.graphics.Canvas
import android.graphics.Color
import android.graphics.Matrix
import android.graphics.pdf.PdfRenderer
import io.shaprint.client.core.domain.model.PrintColorMode
import io.shaprint.client.core.domain.model.PrintOrientation
import io.shaprint.client.core.domain.model.RequestedMedia
import java.io.OutputStream
import kotlin.math.roundToInt

object PdfRasterizer {

    data class PageTransform(
        val scale: Float,
        val dx: Float,
        val dy: Float
    )

    /**
     * Calculates uniform scaling and translation offsets to center a PDF page
     * onto paper dimensions without stretching or distortion.
     */
    fun calculatePrintTransform(
        pageWidth: Float,
        pageHeight: Float,
        paperWidthPx: Int,
        paperHeightPx: Int
    ): PageTransform {
        val scale = minOf(paperWidthPx / pageWidth, paperHeightPx / pageHeight)
        val scaledW = pageWidth * scale
        val scaledH = pageHeight * scale
        val dx = (paperWidthPx - scaledW) / 2f
        val dy = (paperHeightPx - scaledH) / 2f
        return PageTransform(scale, dx, dy)
    }

    /**
     * Calculates the uniform scaling Matrix and translation offsets needed to center
     * a PDF page on target paper dimensions without stretching or distortion.
     */
    fun calculatePrintMatrix(
        pageWidth: Float,
        pageHeight: Float,
        paperWidthPx: Int,
        paperHeightPx: Int
    ): Matrix {
        val transform = calculatePrintTransform(pageWidth, pageHeight, paperWidthPx, paperHeightPx)
        return Matrix().apply {
            postScale(transform.scale, transform.scale)
            postTranslate(transform.dx, transform.dy)
        }
    }

    /**
     * Streams a multi-page PDF document page-by-page directly to a PWG 5102.4 raster output stream.
     * Keeps memory bounded to a single decoded page (~35 MiB) by immediately recycling each page.
     */
    fun streamPdfToPwg(
        pdfRenderer: PdfRenderer,
        media: RequestedMedia,
        colorMode: PrintColorMode,
        out: OutputStream,
        orientation: PrintOrientation = PrintOrientation.PORTRAIT,
        pagesToPrint: List<Int>? = null,
        onPageRendered: ((page: Int, total: Int) -> Unit)? = null
    ) {
        val pageCount = pdfRenderer.pageCount
        if (pageCount == 0) return

        val targetPages = if (pagesToPrint != null && pagesToPrint.isNotEmpty()) {
            pagesToPrint.filter { it in 0 until pageCount }
        } else {
            (0 until pageCount).toList()
        }
        if (targetPages.isEmpty()) return

        // Write the 4-byte stream synchronization header: 'RaS2'
        out.write(PwgRasterConstants.SYNC_WORD)

        val isLandscape = orientation == PrintOrientation.LANDSCAPE
        val paperWidthMm = if (isLandscape) media.heightMm else media.widthMm
        val paperHeightMm = if (isLandscape) media.widthMm else media.heightMm

        val widthPx = ImageRasterizer.mmToPixels(paperWidthMm)
        val heightPx = ImageRasterizer.mmToPixels(paperHeightMm)
        val colors = if (colorMode == PrintColorMode.COLOR) 3 else 1

        val rowPixelsInts = IntArray(widthPx)
        val rowPixelsBytes = ByteArray(widthPx * colors)
        val totalToRender = targetPages.size

        for ((stepIndex, pageIndex) in targetPages.withIndex()) {
            onPageRendered?.invoke(stepIndex + 1, totalToRender)

            val page = pdfRenderer.openPage(pageIndex)
            try {
                // Write 1796-byte PWG header for this page
                val header = PwgEncoder.buildHeader(widthPx, heightPx, colorMode)
                out.write(header)

                // Render page to single-page temporary bitmap with uniform aspect ratio
                val pageBitmap = Bitmap.createBitmap(widthPx, heightPx, Bitmap.Config.ARGB_8888)
                try {
                    val canvas = Canvas(pageBitmap)
                    canvas.drawColor(Color.WHITE)

                    val matrix = calculatePrintMatrix(
                        pageWidth = page.width.toFloat(),
                        pageHeight = page.height.toFloat(),
                        paperWidthPx = widthPx,
                        paperHeightPx = heightPx
                    )

                    page.render(pageBitmap, null, matrix, PdfRenderer.Page.RENDER_MODE_FOR_PRINT)

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
                } finally {
                    pageBitmap.recycle()
                }
            } finally {
                page.close()
            }
        }
    }
}
