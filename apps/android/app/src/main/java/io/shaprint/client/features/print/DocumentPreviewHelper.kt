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
import java.io.FileOutputStream

object DocumentPreviewHelper {

    /**
     * Asynchronously loads or renders a preview bitmap for the currently selected document or test page.
     * Keeps peak memory low by using bounded rendering (~600px width).
     */
    suspend fun loadPreviewBitmap(
        context: Context,
        documentUriString: String?,
        pageIndex: Int,
        media: RequestedMedia,
        colorMode: PrintColorMode,
        printerName: String,
        serverCanonicalAddress: String
    ): Bitmap? = withContext(Dispatchers.IO) {
        if (documentUriString == null) {
            // Test Page preview
            return@withContext try {
                TestPageGenerator.generateTestPageBitmap(
                    printerName = printerName,
                    serverCanonicalAddress = serverCanonicalAddress,
                    media = media,
                    colorMode = colorMode,
                    targetWidth = 600
                )
            } catch (_: Exception) {
                null
            }
        }

        try {
            val uri = Uri.parse(documentUriString)
            val docFile: File = if (uri.scheme == "file" && uri.path != null) {
                File(uri.path!!)
            } else {
                // Read from content resolver into a cache file if needed
                val tempFile = File(context.cacheDir, "shaprint_preview_cache")
                context.contentResolver.openInputStream(uri)?.use { input ->
                    FileOutputStream(tempFile).use { out -> input.copyTo(out) }
                }
                tempFile
            }

            if (!docFile.exists() || docFile.length() == 0L) {
                return@withContext null
            }

            val isPdf = docFile.name.endsWith(".pdf", ignoreCase = true) ||
                    (context.contentResolver.getType(uri) ?: "").contains("pdf", ignoreCase = true)

            if (isPdf) {
                ParcelFileDescriptor.open(docFile, ParcelFileDescriptor.MODE_READ_ONLY).use { pfd ->
                    PdfRenderer(pfd).use { renderer ->
                        if (renderer.pageCount == 0) return@withContext null
                        val safeIndex = pageIndex.coerceIn(0, renderer.pageCount - 1)
                        renderer.openPage(safeIndex).use { page ->
                            val targetWidth = 600
                            val aspectRatio = page.height.toFloat() / page.width.toFloat().coerceAtLeast(1f)
                            val targetHeight = (targetWidth * aspectRatio).toInt().coerceIn(200, 1200)

                            val bitmap = Bitmap.createBitmap(targetWidth, targetHeight, Bitmap.Config.ARGB_8888)
                            val canvas = Canvas(bitmap)
                            canvas.drawColor(Color.WHITE)
                            page.render(bitmap, null, null, PdfRenderer.Page.RENDER_MODE_FOR_DISPLAY)
                            return@withContext bitmap
                        }
                    }
                }
            } else {
                // Decode image with downsampling
                val boundsOpts = BitmapFactory.Options().apply { inJustDecodeBounds = true }
                BitmapFactory.decodeFile(docFile.absolutePath, boundsOpts)

                var sampleSize = 1
                val targetDim = 600
                while (boundsOpts.outWidth / (sampleSize * 2) >= targetDim && boundsOpts.outHeight / (sampleSize * 2) >= targetDim) {
                    sampleSize *= 2
                }

                val decodeOpts = BitmapFactory.Options().apply { inSampleSize = sampleSize }
                return@withContext BitmapFactory.decodeFile(docFile.absolutePath, decodeOpts)
            }
        } catch (_: Exception) {
            null
        }
    }
}
