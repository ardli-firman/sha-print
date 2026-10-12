package io.shaprint.client.core.raster

import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test

class PdfRasterizerTest {

    @Test
    fun calculatePrintTransform_identicalAspectRatio_fillsPaperExactly() {
        // A4 page in points (595.28 x 841.89) onto A4 paper at 300 DPI (2480 x 3508)
        val transform = PdfRasterizer.calculatePrintTransform(
            pageWidth = 595.28f,
            pageHeight = 841.89f,
            paperWidthPx = 2480,
            paperHeightPx = 3508
        )

        // Scale should be uniform
        assertTrue(transform.scale > 0f)
        // Offsets should be practically zero (margins < 1px due to rounding)
        assertEquals(0f, transform.dx, 1.0f)
        assertEquals(0f, transform.dy, 1.0f)
    }

    @Test
    fun calculatePrintTransform_a4OntoLetter_preservesAspectRatioAndCentersHorizontally() {
        // US Letter at 300 DPI: 2550 x 3300. A4 is taller and narrower.
        val transform = PdfRasterizer.calculatePrintTransform(
            pageWidth = 595.28f,
            pageHeight = 841.89f,
            paperWidthPx = 2550,
            paperHeightPx = 3300
        )

        // Scale is constrained by height (3300 / 841.89)
        val expectedScale = 3300f / 841.89f
        assertEquals(expectedScale, transform.scale, 0.01f)
        assertEquals(0f, transform.dy, 0.01f)
        // Scaled width is 595.28 * 3.92 = ~2333px < 2550px
        // dx should be positive and center the document symmetrically
        assertTrue(transform.dx > 100f)
        val scaledWidth = 595.28f * transform.scale
        assertEquals(2550f, scaledWidth + 2 * transform.dx, 0.5f)
    }

    @Test
    fun calculatePrintTransform_portraitOntoLandscape_centersWithoutHorizontalStretching() {
        // Landscape A4 at 300 DPI: 3508 x 2480
        val transform = PdfRasterizer.calculatePrintTransform(
            pageWidth = 595.28f,
            pageHeight = 841.89f,
            paperWidthPx = 3508,
            paperHeightPx = 2480
        )

        // Scale is constrained by landscape paper height (2480 / 841.89)
        val expectedScale = 2480f / 841.89f
        assertEquals(expectedScale, transform.scale, 0.01f)
        assertEquals(0f, transform.dy, 0.01f)

        // Left and right margins should be large and symmetric (no stretching across the 3508px width)
        val scaledWidth = 595.28f * transform.scale
        assertTrue(scaledWidth < 3508f)
        assertEquals(3508f, scaledWidth + 2 * transform.dx, 0.5f)
    }

    @Test
    fun calculatePrintTransform_landscapeOntoPortrait_centersWithoutVerticalStretching() {
        // Landscape document page (841.89 x 595.28) onto Portrait paper (2480 x 3508)
        val transform = PdfRasterizer.calculatePrintTransform(
            pageWidth = 841.89f,
            pageHeight = 595.28f,
            paperWidthPx = 2480,
            paperHeightPx = 3508
        )

        // Scale is constrained by portrait paper width (2480 / 841.89)
        val expectedScale = 2480f / 841.89f
        assertEquals(expectedScale, transform.scale, 0.01f)
        assertEquals(0f, transform.dx, 0.01f)

        // Top and bottom margins should be large and symmetric
        val scaledHeight = 595.28f * transform.scale
        assertTrue(scaledHeight < 3508f)
        assertEquals(3508f, scaledHeight + 2 * transform.dy, 0.5f)
    }
}
