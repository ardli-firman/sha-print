package io.shaprint.client.features.print

import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test

class PageRangeParserTest {

    @Test
    fun parse_emptyOrAll_returnsAllPages() {
        val resultBlank = PageRangeParser.parse("", 5)
        assertTrue(resultBlank is PageRangeResult.Success)
        assertEquals(listOf(0, 1, 2, 3, 4), (resultBlank as PageRangeResult.Success).pageIndices)

        val resultAll = PageRangeParser.parse("all", 3)
        assertTrue(resultAll is PageRangeResult.Success)
        assertEquals(listOf(0, 1, 2), (resultAll as PageRangeResult.Success).pageIndices)
    }

    @Test
    fun parse_singlePage_returnsSingleZeroIndexedPage() {
        val result = PageRangeParser.parse("1", 5)
        assertTrue(result is PageRangeResult.Success)
        assertEquals(listOf(0), (result as PageRangeResult.Success).pageIndices)
    }

    @Test
    fun parse_commaSeparatedList_returnsSortedIndices() {
        // User inputs "1,2,5"
        val result = PageRangeParser.parse("1,2,5", 5)
        assertTrue(result is PageRangeResult.Success)
        assertEquals(listOf(0, 1, 4), (result as PageRangeResult.Success).pageIndices)
    }

    @Test
    fun parse_simpleRange_returnsInclusiveIndices() {
        // User inputs "1-3"
        val result = PageRangeParser.parse("1-3", 5)
        assertTrue(result is PageRangeResult.Success)
        assertEquals(listOf(0, 1, 2), (result as PageRangeResult.Success).pageIndices)
    }

    @Test
    fun parse_combinationRangeAndIndividualPages_returnsDistinctSortedIndices() {
        // User inputs "1-3, 5, 2"
        val result = PageRangeParser.parse("1-3, 5, 2", 6)
        assertTrue(result is PageRangeResult.Success)
        assertEquals(listOf(0, 1, 2, 4), (result as PageRangeResult.Success).pageIndices)
    }

    @Test
    fun parse_pageExceedingTotal_returnsDescriptiveError() {
        val result = PageRangeParser.parse("1, 6", 5)
        assertTrue(result is PageRangeResult.Error)
        val err = (result as PageRangeResult.Error).message
        assertTrue(err.contains("exceeds document total"))
    }

    @Test
    fun parse_pageZero_returnsError() {
        val result = PageRangeParser.parse("0", 5)
        assertTrue(result is PageRangeResult.Error)
        val err = (result as PageRangeResult.Error).message
        assertTrue(err.contains("1 or greater"))
    }

    @Test
    fun parse_invertedRange_returnsError() {
        val result = PageRangeParser.parse("4-2", 5)
        assertTrue(result is PageRangeResult.Error)
        val err = (result as PageRangeResult.Error).message
        assertTrue(err.contains("cannot be greater than end"))
    }

    @Test
    fun parse_nonNumericInput_returnsError() {
        val result = PageRangeParser.parse("abc", 5)
        assertTrue(result is PageRangeResult.Error)
    }

    @Test
    fun formatPageIndices_convertsIndicesToReadableRanges() {
        assertEquals("1-3, 5", PageRangeParser.formatPageIndices(listOf(0, 1, 2, 4)))
        assertEquals("1", PageRangeParser.formatPageIndices(listOf(0)))
        assertEquals("1, 3, 5", PageRangeParser.formatPageIndices(listOf(0, 2, 4)))
    }
}
