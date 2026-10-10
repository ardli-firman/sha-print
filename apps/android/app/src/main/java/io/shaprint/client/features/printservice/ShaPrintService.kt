package io.shaprint.client.features.printservice

import android.printservice.PrintJob
import android.printservice.PrintService
import android.printservice.PrinterDiscoverySession

/**
 * System-wide Android PrintService plugin (ADR 0016, Issue #109).
 * Bridges native Android print dialogs to ShaPrint trusted servers.
 */
class ShaPrintService : PrintService() {

    override fun onCreatePrinterDiscoverySession(): PrinterDiscoverySession? {
        return null // Implemented in Issue #109
    }

    override fun onRequestCancelPrintJob(printJob: PrintJob?) {
        // Implemented in Issue #109
    }

    override fun onPrintJobQueued(printJob: PrintJob?) {
        // Implemented in Issue #109
    }
}
