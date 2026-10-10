package io.shaprint.client.core.raster

import android.graphics.Bitmap
import android.graphics.Canvas
import android.graphics.Color
import android.graphics.Paint
import android.graphics.Rect
import io.shaprint.client.core.domain.model.PrintColorMode
import io.shaprint.client.core.domain.model.RequestedMedia
import java.io.OutputStream
import kotlin.math.roundToInt

object ImageRasterizer {

    /**
     * Converts millimeters to pixels at specified DPI.
     */
    fun mmToPixels(mm: Float, dpi: Int = PwgRasterConstants.RESOLUTION_DPI): Int {
        return (mm / 25.4f * dpi).roundToInt()
    }

    /**
     * Renders an input Bitmap onto a 300 DPI paper canvas (with Fit vs Fill) and streams it as PWG-raster.
     */
    fun streamImageToPwg(
        bitmap: Bitmap,
        media: RequestedMedia,
        colorMode: PrintColorMode,
        fitToPage: Boolean,
        out: OutputStream,
        writeSyncWord: Boolean = true
    ) {
        val widthPx = mmToPixels(media.widthMm)
        val heightPx = mmToPixels(media.heightMm)
        val colors = if (colorMode == PrintColorMode.COLOR) 3 else 1

        if (writeSyncWord) {
            out.write(PwgRasterConstants.SYNC_WORD)
        }

        val header = PwgEncoder.buildHeader(widthPx, heightPx, colorMode)
        out.write(header)

        // Create canvas of page dimensions
        val pageBitmap = Bitmap.createBitmap(widthPx, heightPx, Bitmap.Config.ARGB_8888)
        try {
            val canvas = Canvas(pageBitmap)
            canvas.drawColor(Color.WHITE) // Blank paper background

            // Calculate scaling
            val srcW = bitmap.width.toFloat()
            val srcH = bitmap.height.toFloat()
            val scale = if (fitToPage) {
                minOf(widthPx / srcW, heightPx / srcH)
            } else {
                maxOf(widthPx / srcW, heightPx / srcH)
            }

            val destW = (srcW * scale).roundToInt()
            val destH = (srcH * scale).roundToInt()
            val destX = (widthPx - destW) / 2
            val destY = (heightPx - destH) / 2

            val paint = Paint(Paint.FILTER_BITMAP_FLAG or Paint.DITHER_FLAG)
            canvas.drawBitmap(bitmap, null, Rect(destX, destY, destX + destW, destY + destH), paint)

            // Stream row by row
            val rowPixelsInts = IntArray(widthPx)
            val rowPixelsBytes = ByteArray(widthPx * colors)

            for (y in 0 until heightPx) {
                pageBitmap.getPixels(rowPixelsInts, 0, widthPx, 0, y, widthPx, 1)

                if (colorMode == PrintColorMode.COLOR) {
                    for (x in 0 until widthPx) {
                        val pixel = rowPixelsInts[x]
                        rowPixelsBytes[x * 3] = ((pixel shr 16) and 0xFF).toByte()     // R
                        rowPixelsBytes[x * 3 + 1] = ((pixel shr 8) and 0xFF).toByte()  // G
                        rowPixelsBytes[x * 3 + 2] = (pixel and 0xFF).toByte()         // B
                    }
                } else {
                    for (x in 0 until widthPx) {
                        val pixel = rowPixelsInts[x]
                        val r = (pixel shr 16) and 0xFF
                        val g = (pixel shr 8) and 0xFF
                        val b = pixel and 0xFF
                        // ITU-R BT.601 luminance
                        val gray = (0.299 * r + 0.587 * g + 0.114 * b).roundToInt().coerceIn(0, 255)
                        rowPixelsBytes[x] = gray.toByte()
                    }
                }

                PwgEncoder.encodeRow(rowPixelsBytes, colors, out)
            }
        } finally {
            pageBitmap.recycle()
        }
    }
}
