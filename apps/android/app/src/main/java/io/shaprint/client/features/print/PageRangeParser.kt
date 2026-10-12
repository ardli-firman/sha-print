package io.shaprint.client.features.print

enum class PageRangeMode(val displayName: String) {
    ALL("All pages"),
    CUSTOM("Custom range")
}

sealed class PageRangeResult {
    data class Success(val pageIndices: List<Int>) : PageRangeResult()
    data class Error(val message: String) : PageRangeResult()
}

object PageRangeParser {

    /**
     * Parses and validates a human page range string (e.g. "1-3", "1", "1,2,5", "1-3, 5, 7")
     * into a sorted, distinct list of 0-based page indices.
     */
    fun parse(input: String, totalPages: Int): PageRangeResult {
        val safeTotal = totalPages.coerceAtLeast(1)
        val trimmed = input.trim()

        if (trimmed.isEmpty() || trimmed.equals("all", ignoreCase = true)) {
            return PageRangeResult.Success((0 until safeTotal).toList())
        }

        val parts = trimmed.split(",")
        val resultIndices = mutableSetOf<Int>()

        for (rawPart in parts) {
            val part = rawPart.trim()
            if (part.isEmpty()) continue

            if (part.contains("-")) {
                val rangeParts = part.split("-").map { it.trim() }
                if (rangeParts.size != 2 || rangeParts[0].isEmpty() || rangeParts[1].isEmpty()) {
                    return PageRangeResult.Error("Invalid range format: '$part'. Example: 1-3")
                }

                val start = rangeParts[0].toIntOrNull()
                    ?: return PageRangeResult.Error("Invalid start page in '$part'")
                val end = rangeParts[1].toIntOrNull()
                    ?: return PageRangeResult.Error("Invalid end page in '$part'")

                if (start < 1) {
                    return PageRangeResult.Error("Page number must be 1 or greater ($start)")
                }
                if (start > end) {
                    return PageRangeResult.Error("Range start ($start) cannot be greater than end ($end)")
                }
                if (end > safeTotal) {
                    return PageRangeResult.Error("Page $end exceeds document total of $safeTotal page(s)")
                }

                for (p in start..end) {
                    resultIndices.add(p - 1)
                }
            } else {
                val page = part.toIntOrNull()
                    ?: return PageRangeResult.Error("Invalid page number: '$part'")

                if (page < 1) {
                    return PageRangeResult.Error("Page number must be 1 or greater ($page)")
                }
                if (page > safeTotal) {
                    return PageRangeResult.Error("Page $page exceeds document total of $safeTotal page(s)")
                }

                resultIndices.add(page - 1)
            }
        }

        if (resultIndices.isEmpty()) {
            return PageRangeResult.Error("Please specify at least one page to print")
        }

        return PageRangeResult.Success(resultIndices.sorted())
    }

    /**
     * Formats a list of 0-based page indices into a clean user-facing string (e.g. "1-3, 5, 8").
     */
    fun formatPageIndices(indices: List<Int>): String {
        if (indices.isEmpty()) return ""
        val sorted = indices.distinct().sorted().map { it + 1 }
        val ranges = mutableListOf<String>()

        var start = sorted[0]
        var end = sorted[0]

        for (i in 1 until sorted.size) {
            val current = sorted[i]
            if (current == end + 1) {
                end = current
            } else {
                ranges.add(if (start == end) "$start" else "$start-$end")
                start = current
                end = current
            }
        }
        ranges.add(if (start == end) "$start" else "$start-$end")

        return ranges.joinToString(", ")
    }
}
