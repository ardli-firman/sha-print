package io.shaprint.client.features.print

import android.content.Context
import android.graphics.Bitmap
import android.graphics.BitmapFactory
import android.graphics.Canvas
import android.graphics.Color
import android.graphics.Paint
import android.graphics.Rect
import android.graphics.pdf.PdfRenderer
import android.net.Uri
import android.os.ParcelFileDescriptor
import io.shaprint.client.core.domain.model.PrintColorMode
import io.shaprint.client.core.domain.model.PrintOrientation
import io.shaprint.client.core.domain.model.RequestedMedia
import io.shaprint.client.core.raster.TestPageGenerator
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.withContext
import java.io.File
import java.io.FileInputStream
import java.io.FileOutputStream
import java.util.Collections
import kotlin.math.roundToInt

object DocumentPreviewHelper {

    // Thread-safe in-memory LRU cache holding up to 20 preview bitmaps (~20MB max memory)
    private val previewCache: MutableMap<String, Bitmap> = Collections.synchronizedMap(
        object : LinkedHashMap<String, Bitmap>(20, 0.75f, true) {
            override fun removeEldestEntry(eldest: MutableMap.MutableEntry<String, Bitmap>?): Boolean {
                return size > 20
            }
        }
    )

    fun buildCacheKey(
        documentUriString: String?,
        documentRevision: Long,
        pageIndex: Int,
        media: RequestedMedia,
        orientation: PrintOrientation,
        fitToPage: Boolean,
        targetWidth: Int = 600
    ): String {
        return "${documentUriString ?: "default_test_page"}#rev=$documentRevision#p=$pageIndex#m=${media.name}#o=${orientation.name}#f=$fitToPage#w=$targetWidth"
    }

    fun getCachedBitmap(
        documentUriString: String?,
        documentRevision: Long = 0L,
        pageIndex: Int = 0,
        media: RequestedMedia = RequestedMedia.ISO_A4,
        orientation: PrintOrientation = PrintOrientation.PORTRAIT,
        fitToPage: Boolean = true,
        targetWidth: Int = 600
    ): Bitmap? {
        val key = buildCacheKey(documentUriString, documentRevision, pageIndex, media, orientation, fitToPage, targetWidth)
        return previewCache[key]
    }

    fun clearCache() {
        previewCache.clear()
    }

    /**
     * Inspects the file header bytes to reliably identify PDF vs Image format,
     * regardless of whether the file has an extension or was loaded via ContentProvider.
     */
    fun isPdfFile(file: File, documentName: String? = null): Boolean {
        if (documentName?.endsWith(".pdf", ignoreCase = true) == true) return true
        if (file.name.endsWith(".pdf", ignoreCase = true)) return true

        return try {
            if (!file.exists() || file.length() < 4) return false
            val header = ByteArray(4)
            FileInputStream(file).use { input -> input.read(header) }
            // PDF magic bytes: '%', 'P', 'D', 'F' (0x25, 0x50, 0x44, 0x46)
            header[0] == 0x25.toByte() && header[1] == 0x50.toByte() &&
                    header[2] == 0x44.toByte() && header[3] == 0x46.toByte()
        } catch (_: Exception) {
            false
        }
    }

    /**
     * Asynchronously loads or renders a 1:1 true paper preview bitmap matching the exact
     * dimensions and layout of the physical print output (PdfRasterizer & ImageRasterizer).
     */
    suspend fun loadPreviewBitmap(
        context: Context,
        documentUriString: String?,
        documentName: String? = null,
        documentRevision: Long = 0L,
        pageIndex: Int = 0,
        media: RequestedMedia = RequestedMedia.ISO_A4,
        colorMode: PrintColorMode = PrintColorMode.COLOR,
        orientation: PrintOrientation = PrintOrientation.PORTRAIT,
        fitToPage: Boolean = true,
        printerName: String = "Shared Printer",
        serverCanonicalAddress: String = "LAN Server",
        targetWidth: Int = 600
    ): Bitmap? = withContext(Dispatchers.IO) {
        val cacheKey = buildCacheKey(
            documentUriString,
            documentRevision,
            pageIndex,
            media,
            orientation,
            fitToPage,
            targetWidth
        )
        val existing = previewCache[cacheKey]
        if (existing != null && !existing.isRecycled) {
            return@withContext existing
        }

        // Calculate 1:1 physical paper sheet aspect ratio
        val isLandscape = orientation == PrintOrientation.LANDSCAPE
        val paperWidthMm = if (isLandscape) media.heightMm else media.widthMm
        val paperHeightMm = if (isLandscape) media.widthMm else media.heightMm
        val paperAspect = paperHeightMm / paperWidthMm
        val targetHeight = (targetWidth * paperAspect).roundToInt().coerceIn(200, 2400)

        if (documentUriString == null) {
            // Official ShaPrint Test Page 1:1 preview
            return@withContext try {
                val bitmap = TestPageGenerator.generateTestPageBitmap(
                    printerName = printerName,
                    serverCanonicalAddress = serverCanonicalAddress,
                    media = media,
                    colorMode = colorMode,
                    orientation = orientation,
                    targetWidth = targetWidth
                )
                previewCache[cacheKey] = bitmap
                bitmap
            } catch (_: Exception) {
                null
            }
        }

        try {
            val uri = Uri.parse(documentUriString)
            val docFile: File = if (uri.scheme == "file" && uri.path != null) {
                File(uri.path!!)
            } else {
                val tempFile = File(context.cacheDir, "shaprint_preview_cache.tmp")
                context.contentResolver.openInputStream(uri)?.use { input ->
                    FileOutputStream(tempFile).use { out -> input.copyTo(out) }
                }
                tempFile
            }

            if (!docFile.exists() || docFile.length() == 0L) {
                return@withContext null
            }

            val isPdf = isPdfFile(docFile, documentName)

            if (isPdf) {
                ParcelFileDescriptor.open(docFile, ParcelFileDescriptor.MODE_READ_ONLY).use { pfd ->
                    PdfRenderer(pfd).use { renderer ->
                        if (renderer.pageCount == 0) return@withContext null
                        val safeIndex = pageIndex.coerceIn(0, renderer.pageCount - 1)
                        renderer.openPage(safeIndex).use { page ->
                            // 1:1 WYSIWYG: Render directly onto paper canvas with uniform aspect ratio & centering
                            val paperBitmap = Bitmap.createBitmap(targetWidth, targetHeight, Bitmap.Config.ARGB_8888)
                            val canvas = Canvas(paperBitmap)
                            canvas.drawColor(Color.WHITE)

                            val matrix = io.shaprint.client.core.raster.PdfRasterizer.calculatePrintMatrix(
                                pageWidth = page.width.toFloat(),
                                pageHeight = page.height.toFloat(),
                                paperWidthPx = targetWidth,
                                paperHeightPx = targetHeight
                            )

                            page.render(paperBitmap, null, matrix, PdfRenderer.Page.RENDER_MODE_FOR_PRINT)

                            previewCache[cacheKey] = paperBitmap
                            return@withContext paperBitmap
                        }
                    }
                }
            } else {
                // Decode image with downsampling
                val boundsOpts = BitmapFactory.Options().apply { inJustDecodeBounds = true }
                BitmapFactory.decodeFile(docFile.absolutePath, boundsOpts)

                var sampleSize = 1
                while (boundsOpts.outWidth / (sampleSize * 2) >= targetWidth && boundsOpts.outHeight / (sampleSize * 2) >= targetHeight) {
                    sampleSize *= 2
                }

                val decodeOpts = BitmapFactory.Options().apply { inSampleSize = sampleSize }
                val decoded = BitmapFactory.decodeFile(docFile.absolutePath, decodeOpts)
                    ?: return@withContext null

                try {
                    // 1:1 WYSIWYG: Render onto paper canvas using exact ImageRasterizer layout & fitToPage
                    val paperBitmap = Bitmap.createBitmap(targetWidth, targetHeight, Bitmap.Config.ARGB_8888)
                    val canvas = Canvas(paperBitmap)
                    canvas.drawColor(Color.WHITE)

                    val srcW = decoded.width.toFloat()
                    val srcH = decoded.height.toFloat()
                    val scale = if (fitToPage) {
                        minOf(targetWidth / srcW, targetHeight / srcH)
                    } else {
                        maxOf(targetWidth / srcW, targetHeight / srcH)
                    }

                    val destW = (srcW * scale).roundToInt()
                    val destH = (srcH * scale).roundToInt()
                    val destX = (targetWidth - destW) / 2
                    val destY = (targetHeight - destH) / 2

                    val paint = Paint(Paint.FILTER_BITMAP_FLAG or Paint.DITHER_FLAG)
                    canvas.drawBitmap(decoded, null, Rect(destX, destY, destX + destW, destY + destH), paint)

                    previewCache[cacheKey] = paperBitmap
                    return@withContext paperBitmap
                } finally {
                    decoded.recycle()
                }
            }
        } catch (_: Exception) {
            null
        }
    }
}
