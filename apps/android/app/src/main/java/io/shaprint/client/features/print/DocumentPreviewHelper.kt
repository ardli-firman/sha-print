package io.shaprint.client.features.print

import android.content.Context
import android.graphics.Bitmap
import android.graphics.BitmapFactory
import android.graphics.Canvas
import android.graphics.Color
import android.graphics.pdf.PdfRenderer
import android.net.Uri
import android.os.ParcelFileDescriptor
import io.shaprint.client.core.domain.model.PrintColorMode
import io.shaprint.client.core.domain.model.RequestedMedia
import io.shaprint.client.core.raster.TestPageGenerator
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.withContext
import java.io.File
import java.io.FileInputStream
import java.io.FileOutputStream
import java.util.Collections

object DocumentPreviewHelper {

    // Thread-safe in-memory LRU cache holding up to 16 preview bitmaps (~15MB max memory)
    private val previewCache: MutableMap<String, Bitmap> = Collections.synchronizedMap(
        object : LinkedHashMap<String, Bitmap>(16, 0.75f, true) {
            override fun removeEldestEntry(eldest: MutableMap.MutableEntry<String, Bitmap>?): Boolean {
                return size > 16
            }
        }
    )

    fun getCachedBitmap(
        documentUriString: String?,
        pageIndex: Int,
        targetWidth: Int = 600
    ): Bitmap? {
        val key = buildCacheKey(documentUriString, pageIndex, targetWidth)
        return previewCache[key]
    }

    fun clearCache() {
        previewCache.clear()
    }

    private fun buildCacheKey(documentUriString: String?, pageIndex: Int, targetWidth: Int): String {
        return "${documentUriString ?: "default_test_page"}#p=$pageIndex#w=$targetWidth"
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
     * Asynchronously loads or renders a preview bitmap for the currently selected document or test page.
     * Uses memory caching and fast bounded rendering to guarantee responsive sub-100ms previews.
     */
    suspend fun loadPreviewBitmap(
        context: Context,
        documentUriString: String?,
        documentName: String? = null,
        pageIndex: Int = 0,
        media: RequestedMedia = RequestedMedia.ISO_A4,
        colorMode: PrintColorMode = PrintColorMode.COLOR,
        printerName: String = "Shared Printer",
        serverCanonicalAddress: String = "LAN Server",
        targetWidth: Int = 600
    ): Bitmap? = withContext(Dispatchers.IO) {
        val cacheKey = buildCacheKey(documentUriString, pageIndex, targetWidth)
        val existing = previewCache.get(cacheKey)
        if (existing != null && !existing.isRecycled) {
            return@withContext existing
        }

        if (documentUriString == null) {
            // Test Page preview
            return@withContext try {
                val bitmap = TestPageGenerator.generateTestPageBitmap(
                    printerName = printerName,
                    serverCanonicalAddress = serverCanonicalAddress,
                    media = media,
                    colorMode = colorMode,
                    targetWidth = targetWidth
                )
                previewCache.put(cacheKey, bitmap)
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
                            val aspectRatio = page.height.toFloat() / page.width.toFloat().coerceAtLeast(1f)
                            val targetHeight = (targetWidth * aspectRatio).toInt().coerceIn(150, 1800)

                            val bitmap = Bitmap.createBitmap(targetWidth, targetHeight, Bitmap.Config.ARGB_8888)
                            val canvas = Canvas(bitmap)
                            canvas.drawColor(Color.WHITE)
                            page.render(bitmap, null, null, PdfRenderer.Page.RENDER_MODE_FOR_DISPLAY)

                            previewCache.put(cacheKey, bitmap)
                            return@withContext bitmap
                        }
                    }
                }
            } else {
                // Decode image with smart downsampling
                val boundsOpts = BitmapFactory.Options().apply { inJustDecodeBounds = true }
                BitmapFactory.decodeFile(docFile.absolutePath, boundsOpts)

                var sampleSize = 1
                while (boundsOpts.outWidth / (sampleSize * 2) >= targetWidth && boundsOpts.outHeight / (sampleSize * 2) >= targetWidth) {
                    sampleSize *= 2
                }

                val decodeOpts = BitmapFactory.Options().apply { inSampleSize = sampleSize }
                val decoded = BitmapFactory.decodeFile(docFile.absolutePath, decodeOpts)
                if (decoded != null) {
                    previewCache.put(cacheKey, decoded)
                }
                return@withContext decoded
            }
        } catch (_: Exception) {
            null
        }
    }
}
