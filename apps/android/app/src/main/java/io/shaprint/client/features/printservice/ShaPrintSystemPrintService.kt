package io.shaprint.client.features.printservice

import android.graphics.pdf.PdfRenderer
import android.os.ParcelFileDescriptor
import android.print.PrintAttributes
import android.print.PrinterCapabilitiesInfo
import android.print.PrinterId
import android.print.PrinterInfo
import android.printservice.PrintJob
import android.printservice.PrintService
import android.printservice.PrinterDiscoverySession
import io.shaprint.client.core.domain.model.DuplexMode
import io.shaprint.client.core.domain.model.PrintColorMode
import io.shaprint.client.core.domain.model.SharedPrinter
import io.shaprint.client.core.network.DefaultIppsClient
import io.shaprint.client.core.network.IppsClient
import io.shaprint.client.core.raster.PdfRasterizer
import io.shaprint.client.core.raster.PwgRasterConstants
import io.shaprint.client.core.security.EncryptedTrustStore
import io.shaprint.client.core.security.TrustStore
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.cancel
import kotlinx.coroutines.launch
import java.io.File
import java.io.FileOutputStream

/**
 * Android System Print Service (ADR 0016) exposing shared printers from Trusted Servers
 * to Chrome, Gmail, Google Docs, and all system-wide Android Print dialogues.
 */
class ShaPrintSystemPrintService : PrintService() {

    private val serviceScope = CoroutineScope(SupervisorJob() + Dispatchers.IO)
    private lateinit var trustStore: TrustStore
    private val ippsClient: IppsClient = DefaultIppsClient()

    override fun onCreate() {
        super.onCreate()
        trustStore = EncryptedTrustStore(applicationContext)
    }

    override fun onDestroy() {
        serviceScope.cancel()
        super.onDestroy()
    }

    override fun onCreatePrinterDiscoverySession(): PrinterDiscoverySession {
        return ShaPrintDiscoverySession(this, serviceScope, trustStore, ippsClient)
    }

    override fun onRequestCancelPrintJob(printJob: PrintJob) {
        printJob.cancel()
    }

    override fun onPrintJobQueued(printJob: PrintJob) {
        if (!printJob.isQueued) return
        printJob.start()

        val info = printJob.info
        val printerId = info.printerId ?: run {
            printJob.fail("Missing target printer ID")
            return
        }

        val attrs = info.attributes
        val mediaSize = attrs.mediaSize
        val colorModeCode = attrs.colorMode
        val duplexCode = attrs.duplexMode

        val domainRequest = PrintServiceMapper.toPrintJobRequest(
            printerLocalId = printerId.localId,
            jobLabel = info.label ?: "Android Print Job",
            copies = info.copies,
            mediaId = mediaSize?.id,
            isColor = colorModeCode == PrintAttributes.COLOR_MODE_COLOR,
            duplexModeCode = duplexCode,
            isPortrait = mediaSize?.isPortrait ?: true
        ) ?: run {
            printJob.fail("Invalid ShaPrint printer identifier: ${printerId.localId}")
            return
        }

        // Copy system spooler ParcelFileDescriptor to temporary file so PdfRenderer can seek
        val dataPfd = printJob.document.data ?: run {
            printJob.fail("Empty print document data")
            return
        }

        val tempPdfFile = File.createTempFile("shaprint_spool_", ".pdf", cacheDir)
        try {
            ParcelFileDescriptor.AutoCloseInputStream(dataPfd).use { input ->
                FileOutputStream(tempPdfFile).use { output ->
                    input.copyTo(output)
                }
            }
        } catch (e: Exception) {
            tempPdfFile.delete()
            printJob.fail("Failed to read spooled PDF document: ${e.message}")
            return
        }

        serviceScope.launch {
            try {
                val trustedServer = trustStore.findByCanonicalAddress(domainRequest.serverCanonicalAddress)
                if (trustedServer == null) {
                    printJob.fail("Server ${domainRequest.serverCanonicalAddress} is no longer trusted.")
                    return@launch
                }

                val pfd = ParcelFileDescriptor.open(tempPdfFile, ParcelFileDescriptor.MODE_READ_ONLY)
                val pdfRenderer = PdfRenderer(pfd)

                val submissionResult = try {
                    ippsClient.submitPrintJob(
                        server = trustedServer,
                        jobRequest = domainRequest
                    ) { outputStream ->
                        PdfRasterizer.streamPdfToPwg(
                            pdfRenderer = pdfRenderer,
                            media = domainRequest.media,
                            colorMode = domainRequest.colorMode,
                            out = outputStream
                        )
                    }
                } finally {
                    pdfRenderer.close()
                    pfd.close()
                }

                if (submissionResult.isSuccess) {
                    printJob.complete()
                } else {
                    printJob.fail(submissionResult.errorMessage ?: "Server rejected print job.")
                }
            } catch (e: Exception) {
                printJob.fail(e.message ?: "Error streaming print job to server.")
            } finally {
                tempPdfFile.delete()
            }
        }
    }

    private class ShaPrintDiscoverySession(
        private val service: ShaPrintSystemPrintService,
        private val scope: CoroutineScope,
        private val trustStore: TrustStore,
        private val ippsClient: IppsClient
    ) : PrinterDiscoverySession() {

        private val cachedPrinters = mutableMapOf<String, SharedPrinter>()

        override fun onStartPrinterDiscovery(priorityList: MutableList<PrinterId>) {
            refreshPrinters()
        }

        override fun onStopPrinterDiscovery() {}

        override fun onValidatePrinters(printerIds: MutableList<PrinterId>) {
            refreshPrinters()
        }

        override fun onStartPrinterStateTracking(printerId: PrinterId) {
            val sharedPrinter = cachedPrinters[printerId.localId]
            if (sharedPrinter != null) {
                val printerInfo = buildPrinterInfo(printerId, sharedPrinter)
                addPrinters(listOf(printerInfo))
            } else {
                refreshPrinters()
            }
        }

        override fun onStopPrinterStateTracking(printerId: PrinterId) {}

        override fun onDestroy() {
            cachedPrinters.clear()
        }

        private fun refreshPrinters() {
            scope.launch {
                val servers = trustStore.getTrustedServers()
                val discoveredInfos = mutableListOf<PrinterInfo>()

                for (server in servers) {
                    try {
                        val printers = ippsClient.queryPrinters(server)
                        for (printer in printers) {
                            val localId = PrintServiceMapper.formatPrinterLocalId(
                                server.canonicalAddress,
                                printer.name
                            )
                            cachedPrinters[localId] = printer
                            val printerId = service.generatePrinterId(localId)
                            discoveredInfos.add(buildPrinterInfo(printerId, printer))
                        }
                    } catch (_: Exception) {
                        // Offline or unreachable server skipped
                    }
                }

                if (discoveredInfos.isNotEmpty()) {
                    addPrinters(discoveredInfos)
                }
            }
        }

        private fun buildPrinterInfo(printerId: PrinterId, printer: SharedPrinter): PrinterInfo {
            val capsBuilder = PrinterCapabilitiesInfo.Builder(printerId)

            val supportedMedia = printer.capabilities.supportedMedia
            supportedMedia.forEachIndexed { index, media ->
                val dims = PrintServiceMapper.toSystemMediaDimensions(media)
                val mediaSize = PrintAttributes.MediaSize(
                    dims.id,
                    dims.label,
                    dims.widthMils,
                    dims.heightMils
                )
                capsBuilder.addMediaSize(mediaSize, index == 0)
            }

            capsBuilder.addResolution(
                PrintAttributes.Resolution(
                    "pwg_300dpi",
                    "300 DPI PWG Raster",
                    PwgRasterConstants.RESOLUTION_DPI,
                    PwgRasterConstants.RESOLUTION_DPI
                ),
                true
            )

            var colorModesMask = PrintAttributes.COLOR_MODE_MONOCHROME
            var defaultColorMode = PrintAttributes.COLOR_MODE_MONOCHROME
            if (printer.capabilities.supportedColorModes.contains(PrintColorMode.COLOR)) {
                colorModesMask = colorModesMask or PrintAttributes.COLOR_MODE_COLOR
                defaultColorMode = PrintAttributes.COLOR_MODE_COLOR
            }
            capsBuilder.setColorModes(colorModesMask, defaultColorMode)

            var duplexMask = PrintAttributes.DUPLEX_MODE_NONE
            if (printer.capabilities.supportedDuplexModes.contains(DuplexMode.TWO_SIDED_LONG_EDGE)) {
                duplexMask = duplexMask or PrintAttributes.DUPLEX_MODE_LONG_EDGE
            }
            if (printer.capabilities.supportedDuplexModes.contains(DuplexMode.TWO_SIDED_SHORT_EDGE)) {
                duplexMask = duplexMask or PrintAttributes.DUPLEX_MODE_SHORT_EDGE
            }
            capsBuilder.setDuplexModes(duplexMask, PrintAttributes.DUPLEX_MODE_NONE)
            capsBuilder.setMinMargins(PrintAttributes.Margins.NO_MARGINS)

            val status = if (printer.isOnline) PrinterInfo.STATUS_IDLE else PrinterInfo.STATUS_UNAVAILABLE

            return PrinterInfo.Builder(printerId, printer.name, status)
                .setDescription("ShaPrint (${printer.serverCanonicalAddress})")
                .setCapabilities(capsBuilder.build())
                .build()
        }
    }
}
