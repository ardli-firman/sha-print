package io.shaprint.client.core.raster

import android.graphics.Bitmap
import android.graphics.Canvas
import android.graphics.Color
import android.graphics.Paint
import android.graphics.RectF
import io.shaprint.client.core.domain.model.PrintColorMode
import io.shaprint.client.core.domain.model.RequestedMedia
import java.io.OutputStream
import java.text.SimpleDateFormat
import java.util.Date
import java.util.Locale

object TestPageGenerator {

    /**
     * Generates an official ShaPrint Test Page and streams it directly to PWG-raster format.
     * Useful for testing network connectivity, certificate pinning, and driver raster compatibility.
     */
    fun streamTestPageToPwg(
        printerName: String,
        serverCanonicalAddress: String,
        media: RequestedMedia,
        colorMode: PrintColorMode,
        out: OutputStream
    ) {
        val widthPx = ImageRasterizer.mmToPixels(media.widthMm)
        val heightPx = ImageRasterizer.mmToPixels(media.heightMm)

        val bitmap = Bitmap.createBitmap(widthPx, heightPx, Bitmap.Config.ARGB_8888)
        try {
            val canvas = Canvas(bitmap)
            canvas.drawColor(Color.WHITE)

            val scale = widthPx / 2480f
            val margin = 80f * scale

            // Outer border
            val borderPaint = Paint().apply {
                color = Color.rgb(180, 180, 180)
                style = Paint.Style.STROKE
                strokeWidth = 3f * scale
            }
            canvas.drawRect(margin, margin, widthPx - margin, heightPx - margin, borderPaint)

            // Header banner
            val headerPaint = Paint().apply {
                color = Color.rgb(24, 76, 120)
                isAntiAlias = true
            }
            val bannerHeight = 180f * scale
            canvas.drawRoundRect(
                RectF(margin + 20f * scale, margin + 20f * scale, widthPx - margin - 20f * scale, margin + bannerHeight),
                20f * scale,
                20f * scale,
                headerPaint
            )

            // Title text
            val titlePaint = Paint().apply {
                color = Color.WHITE
                textSize = 64f * scale
                isFakeBoldText = true
                isAntiAlias = true
            }
            canvas.drawText("ShaPrint Test Page", margin + 60f * scale, margin + 95f * scale, titlePaint)

            val subtitlePaint = Paint().apply {
                color = Color.rgb(200, 225, 255)
                textSize = 32f * scale
                isAntiAlias = true
            }
            canvas.drawText(
                "Secure IPPS LAN Print Verification • Android Client",
                margin + 60f * scale,
                margin + 145f * scale,
                subtitlePaint
            )

            // Details section
            var currentY = margin + bannerHeight + 80f * scale
            val labelPaint = Paint().apply {
                color = Color.rgb(80, 80, 80)
                textSize = 36f * scale
                isFakeBoldText = true
                isAntiAlias = true
            }
            val valuePaint = Paint().apply {
                color = Color.rgb(20, 20, 20)
                textSize = 36f * scale
                isAntiAlias = true
            }

            fun drawInfoRow(label: String, value: String) {
                canvas.drawText(label, margin + 60f * scale, currentY, labelPaint)
                canvas.drawText(value, margin + 460f * scale, currentY, valuePaint)
                currentY += 56f * scale
            }

            drawInfoRow("Printer Queue:", printerName)
            drawInfoRow("Server Address:", serverCanonicalAddress)
            drawInfoRow("Paper Size:", "${media.displayName} (${media.widthMm.toInt()} × ${media.heightMm.toInt()} mm)")
            drawInfoRow("Color Mode:", colorMode.displayName)
            drawInfoRow("Raster Format:", "PWG 5102.4 (300 DPI PackBits)")
            val dateFormat = SimpleDateFormat("yyyy-MM-dd HH:mm:ss", Locale.US)
            drawInfoRow("Timestamp:", dateFormat.format(Date()))

            currentY += 40f * scale

            // Separator line
            val linePaint = Paint().apply {
                color = Color.rgb(220, 220, 220)
                strokeWidth = 2f * scale
            }
            canvas.drawLine(margin + 40f * scale, currentY, widthPx - margin - 40f * scale, currentY, linePaint)
            currentY += 60f * scale

            // Color calibration swatches
            val sectionPaint = Paint().apply {
                color = Color.rgb(30, 30, 30)
                textSize = 40f * scale
                isFakeBoldText = true
                isAntiAlias = true
            }
            canvas.drawText("Color & Greyscale Calibration", margin + 60f * scale, currentY, sectionPaint)
            currentY += 50f * scale

            val swatchWidth = (widthPx - 2 * margin - 120f * scale) / 7f
            val swatchHeight = 100f * scale
            val swatchColors = if (colorMode == PrintColorMode.COLOR) {
                listOf(
                    Color.rgb(0, 168, 224),   // Cyan
                    Color.rgb(236, 0, 140),   // Magenta
                    Color.rgb(255, 237, 0),   // Yellow
                    Color.rgb(0, 0, 0),       // Black
                    Color.rgb(237, 28, 36),   // Red
                    Color.rgb(0, 166, 81),    // Green
                    Color.rgb(46, 49, 146)    // Blue
                )
            } else {
                listOf(
                    Color.rgb(230, 230, 230),
                    Color.rgb(190, 190, 190),
                    Color.rgb(150, 150, 150),
                    Color.rgb(110, 110, 110),
                    Color.rgb(70, 70, 70),
                    Color.rgb(40, 40, 40),
                    Color.rgb(0, 0, 0)
                )
            }

            val swatchPaint = Paint().apply { isAntiAlias = true }
            swatchColors.forEachIndexed { index, c ->
                val left = margin + 60f * scale + index * swatchWidth
                swatchPaint.color = c
                canvas.drawRoundRect(
                    RectF(left, currentY, left + swatchWidth - 10f * scale, currentY + swatchHeight),
                    8f * scale,
                    8f * scale,
                    swatchPaint
                )
            }

            currentY += swatchHeight + 40f * scale

            // Greyscale gradient ramp (10 steps)
            val rampSteps = 10
            val rampWidth = (widthPx - 2 * margin - 120f * scale) / rampSteps.toFloat()
            val rampHeight = 60f * scale
            for (step in 0 until rampSteps) {
                val grayVal = (255 * (1f - step.toFloat() / (rampSteps - 1))).toInt()
                swatchPaint.color = Color.rgb(grayVal, grayVal, grayVal)
                val left = margin + 60f * scale + step * rampWidth
                canvas.drawRect(left, currentY, left + rampWidth, currentY + rampHeight, swatchPaint)
            }

            // Footer
            val footerPaint = Paint().apply {
                color = Color.rgb(130, 130, 130)
                textSize = 28f * scale
                isAntiAlias = true
                textAlign = Paint.Align.CENTER
            }
            canvas.drawText(
                "If you can read this text and see the calibration blocks, your ShaPrint connection is working perfectly.",
                widthPx / 2f,
                heightPx - margin - 40f * scale,
                footerPaint
            )

            // Stream rendered test page to PWG-raster
            ImageRasterizer.streamImageToPwg(
                bitmap = bitmap,
                media = media,
                colorMode = colorMode,
                fitToPage = true,
                out = out
            )
        } finally {
            bitmap.recycle()
        }
    }
}
