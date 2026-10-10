package io.shaprint.client.core.ipp

import io.shaprint.client.core.domain.model.DuplexMode
import io.shaprint.client.core.domain.model.PrintColorMode
import io.shaprint.client.core.domain.model.PrinterCapabilities
import io.shaprint.client.core.domain.model.RequestedMedia
import io.shaprint.client.core.domain.model.SharedPrinter
import java.io.ByteArrayOutputStream
import java.io.DataOutputStream
import java.nio.ByteBuffer

object IppMessage {

    /**
     * Builds an IPP request datagram for `Get-Printers` (0x4002) or `Get-Printer-Attributes` (0x000B).
     */
    fun buildGetPrintersRequest(
        printerUri: String,
        networkChannel: String? = null,
        requestId: Int = 1,
        operationId: Short = IppConstants.OP_GET_PRINTER_ATTRIBUTES
    ): ByteArray {
        val baos = ByteArrayOutputStream()
        val dos = DataOutputStream(baos)

        // Header: Version 2.0, Operation ID, Request ID
        dos.writeShort(IppConstants.IPP_VERSION_2_0.toInt())
        dos.writeShort(operationId.toInt())
        dos.writeInt(requestId)

        // Group 1: Operation Attributes
        dos.writeByte(IppConstants.TAG_OPERATION_ATTRIBUTES.toInt())

        writeStringAttribute(dos, IppConstants.TAG_CHARSET, IppConstants.ATTR_ATTRIBUTES_CHARSET, "utf-8")
        writeStringAttribute(dos, IppConstants.TAG_NATURAL_LANGUAGE, IppConstants.ATTR_ATTRIBUTES_NATURAL_LANGUAGE, "en")
        writeStringAttribute(dos, IppConstants.TAG_URI, IppConstants.ATTR_PRINTER_URI, printerUri)
        writeStringAttribute(dos, IppConstants.TAG_NAME_WITHOUT_LANGUAGE, IppConstants.ATTR_REQUESTING_USER_NAME, "shaprint-android")

        if (!networkChannel.isNullOrBlank()) {
            writeStringAttribute(dos, IppConstants.TAG_KEYWORD, IppConstants.ATTR_NETWORK_CHANNEL, networkChannel)
        }

        // End of Attributes
        dos.writeByte(IppConstants.TAG_END_OF_ATTRIBUTES.toInt())

        return baos.toByteArray()
    }

    private fun writeStringAttribute(dos: DataOutputStream, tag: Byte, name: String, value: String) {
        dos.writeByte(tag.toInt())
        val nameBytes = name.toByteArray(Charsets.UTF_8)
        dos.writeShort(nameBytes.size)
        dos.write(nameBytes)

        val valueBytes = value.toByteArray(Charsets.UTF_8)
        dos.writeShort(valueBytes.size)
        dos.write(valueBytes)
    }

    data class ParsedIppResponse(
        val statusCode: Short,
        val requestId: Int,
        val printers: List<SharedPrinter>
    )

    /**
     * Parses an IPP binary response into status code and list of SharedPrinters with dynamic capabilities.
     */
    fun parseResponse(data: ByteArray, serverCanonicalAddress: String): ParsedIppResponse {
        if (data.size < 8) {
            return ParsedIppResponse(
                statusCode = IppConstants.STATUS_CLIENT_ERROR_NOT_FOUND,
                requestId = 0,
                printers = emptyList()
            )
        }

        val buffer = ByteBuffer.wrap(data)
        buffer.short // version
        val statusCode = buffer.short
        val requestId = buffer.int

        val printers = mutableListOf<SharedPrinter>()

        var currentGroup: Byte = 0
        var currentPrinterName: String? = null
        val currentMedia = mutableListOf<RequestedMedia>()
        val currentColorModes = mutableListOf<PrintColorMode>()
        val currentDuplexModes = mutableListOf<DuplexMode>()
        var currentMaxCopies = 99
        var lastAttributeName = ""

        fun commitCurrentPrinter() {
            val name = currentPrinterName
            if (name != null) {
                printers.add(
                    SharedPrinter(
                        name = name,
                        serverCanonicalAddress = serverCanonicalAddress,
                        isOnline = true,
                        capabilities = PrinterCapabilities(
                            supportedMedia = if (currentMedia.isNotEmpty()) currentMedia.toList() else listOf(RequestedMedia.ISO_A4),
                            supportedColorModes = if (currentColorModes.isNotEmpty()) currentColorModes.toList() else listOf(PrintColorMode.COLOR),
                            supportedDuplexModes = if (currentDuplexModes.isNotEmpty()) currentDuplexModes.toList() else listOf(DuplexMode.ONE_SIDED),
                            maxCopies = currentMaxCopies
                        )
                    )
                )
            }
            currentPrinterName = null
            currentMedia.clear()
            currentColorModes.clear()
            currentDuplexModes.clear()
            currentMaxCopies = 99
        }

        while (buffer.hasRemaining()) {
            val tag = buffer.get()
            if (tag == IppConstants.TAG_END_OF_ATTRIBUTES) {
                commitCurrentPrinter()
                break
            }

            if (tag in 0x01..0x05) {
                // Group delimiter
                if (tag == IppConstants.TAG_PRINTER_ATTRIBUTES && currentPrinterName != null) {
                    commitCurrentPrinter()
                }
                currentGroup = tag
                continue
            }

            // Attribute
            if (buffer.remaining() < 2) break
            val nameLen = buffer.short.toInt() and 0xFFFF
            val attrName = if (nameLen > 0 && buffer.remaining() >= nameLen) {
                val nameBytes = ByteArray(nameLen)
                buffer.get(nameBytes)
                String(nameBytes, Charsets.UTF_8).also { lastAttributeName = it }
            } else {
                lastAttributeName // 1setOf additional value
            }

            if (buffer.remaining() < 2) break
            val valueLen = buffer.short.toInt() and 0xFFFF
            if (buffer.remaining() < valueLen) break

            val valueBytes = ByteArray(valueLen)
            buffer.get(valueBytes)

            if (currentGroup == IppConstants.TAG_PRINTER_ATTRIBUTES) {
                val stringVal = String(valueBytes, Charsets.UTF_8)
                when (attrName) {
                    "printer-name" -> {
                        currentPrinterName = stringVal
                    }
                    "media-supported", "media-ready" -> {
                        RequestedMedia.fromIppName(stringVal)?.let { media ->
                            if (!currentMedia.contains(media)) currentMedia.add(media)
                        }
                    }
                    "print-color-mode-supported" -> {
                        when (stringVal) {
                            "color" -> if (!currentColorModes.contains(PrintColorMode.COLOR)) currentColorModes.add(PrintColorMode.COLOR)
                            "monochrome", "bi-level" -> if (!currentColorModes.contains(PrintColorMode.MONOCHROME)) currentColorModes.add(PrintColorMode.MONOCHROME)
                        }
                    }
                    "sides-supported" -> {
                        when (stringVal) {
                            "one-sided" -> if (!currentDuplexModes.contains(DuplexMode.ONE_SIDED)) currentDuplexModes.add(DuplexMode.ONE_SIDED)
                            "two-sided-long-edge" -> if (!currentDuplexModes.contains(DuplexMode.TWO_SIDED_LONG_EDGE)) currentDuplexModes.add(DuplexMode.TWO_SIDED_LONG_EDGE)
                            "two-sided-short-edge" -> if (!currentDuplexModes.contains(DuplexMode.TWO_SIDED_SHORT_EDGE)) currentDuplexModes.add(DuplexMode.TWO_SIDED_SHORT_EDGE)
                        }
                    }
                    "copies-supported" -> {
                        if (valueLen == 8) {
                            // rangeOfInteger: lowerBound (4 bytes), upperBound (4 bytes)
                            val valBuf = ByteBuffer.wrap(valueBytes)
                            val lower = valBuf.int
                            val upper = valBuf.int
                            currentMaxCopies = upper
                        } else if (valueLen == 4) {
                            currentMaxCopies = ByteBuffer.wrap(valueBytes).int
                        }
                    }
                }
            }
        }

        return ParsedIppResponse(
            statusCode = statusCode,
            requestId = requestId,
            printers = printers
        )
    }
}
