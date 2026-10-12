package io.shaprint.client.features.print

import io.shaprint.client.core.domain.model.PrintOrientation
import io.shaprint.client.core.domain.model.RequestedMedia
import org.junit.After
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNotEquals
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Before
import org.junit.Test
import java.io.File
import java.io.FileOutputStream

class DocumentPreviewHelperTest {

    private lateinit var tempDir: File

    @Before
    fun setUp() {
        tempDir = File.createTempFile("shaprint_preview_test", "").apply {
            delete()
            mkdir()
        }
        DocumentPreviewHelper.clearCache()
    }

    @After
    fun tearDown() {
        tempDir.deleteRecursively()
        DocumentPreviewHelper.clearCache()
    }

    @Test
    fun isPdfFile_detectsPdfByExtension() {
        val pdfFile = File(tempDir, "contract.pdf").apply {
            writeBytes(byteArrayOf(1, 2, 3))
        }
        assertTrue(DocumentPreviewHelper.isPdfFile(pdfFile, "contract.pdf"))
    }

    @Test
    fun isPdfFile_detectsPdfByDocumentNameFallback() {
        val noExtFile = File(tempDir, "cached_file_without_ext").apply {
            writeBytes(byteArrayOf(1, 2, 3))
        }
        assertTrue(DocumentPreviewHelper.isPdfFile(noExtFile, "Annual-Report.PDF"))
    }

    @Test
    fun isPdfFile_detectsPdfByMagicBytesEvenWithoutExtension() {
        val noExtFile = File(tempDir, "shaprint_doc_123456").apply {
            FileOutputStream(this).use { out ->
                // PDF 1.4 header bytes: %PDF-1.4
                out.write(byteArrayOf(0x25, 0x50, 0x44, 0x46, 0x2D, 0x31, 0x2E, 0x34))
            }
        }
        assertTrue(DocumentPreviewHelper.isPdfFile(noExtFile, "unnamed_upload"))
    }

    @Test
    fun isPdfFile_returnsFalseForNonPdfImageBytes() {
        val imageFile = File(tempDir, "shaprint_doc_654321").apply {
            FileOutputStream(this).use { out ->
                // PNG header bytes: 0x89, 'P', 'N', 'G'
                out.write(byteArrayOf(0x89.toByte(), 0x50, 0x4E, 0x47))
            }
        }
        assertFalse(DocumentPreviewHelper.isPdfFile(imageFile, "photo.png"))
    }

    @Test
    fun cacheKey_changesWhenDocumentOrSettingsChange() {
        val keyDoc1 = DocumentPreviewHelper.buildCacheKey(
            documentUriString = "file:///cache/doc1.pdf",
            documentRevision = 1L,
            pageIndex = 0,
            media = RequestedMedia.ISO_A4,
            orientation = PrintOrientation.PORTRAIT,
            fitToPage = true
        )
        val keyDoc2 = DocumentPreviewHelper.buildCacheKey(
            documentUriString = "file:///cache/doc2.pdf",
            documentRevision = 2L,
            pageIndex = 0,
            media = RequestedMedia.ISO_A4,
            orientation = PrintOrientation.PORTRAIT,
            fitToPage = true
        )
        val keyLandscape = DocumentPreviewHelper.buildCacheKey(
            documentUriString = "file:///cache/doc1.pdf",
            documentRevision = 1L,
            pageIndex = 0,
            media = RequestedMedia.ISO_A4,
            orientation = PrintOrientation.LANDSCAPE,
            fitToPage = true
        )
        val keyFitFalse = DocumentPreviewHelper.buildCacheKey(
            documentUriString = "file:///cache/doc1.pdf",
            documentRevision = 1L,
            pageIndex = 0,
            media = RequestedMedia.ISO_A4,
            orientation = PrintOrientation.PORTRAIT,
            fitToPage = false
        )

        assertNotEquals(keyDoc1, keyDoc2)
        assertNotEquals(keyDoc1, keyLandscape)
        assertNotEquals(keyDoc1, keyFitFalse)
    }

    @Test
    fun cacheEviction_clearsBitmaps() {
        assertNull(DocumentPreviewHelper.getCachedBitmap("nonexistent_uri", 0L, 0))
        DocumentPreviewHelper.clearCache()
        assertNull(DocumentPreviewHelper.getCachedBitmap(null, 0L, 0))
    }
}
