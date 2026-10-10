package io.shaprint.client.ui.navigation

import android.content.Intent
import android.graphics.BitmapFactory
import android.graphics.pdf.PdfRenderer
import android.net.Uri
import android.os.ParcelFileDescriptor
import android.provider.OpenableColumns
import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.runtime.Composable
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.runtime.remember
import androidx.compose.ui.platform.LocalContext
import androidx.navigation.NavHostController
import androidx.navigation.compose.NavHost
import androidx.navigation.compose.composable
import androidx.navigation.compose.rememberNavController
import io.shaprint.client.core.network.DefaultIppsClient
import io.shaprint.client.core.network.DefaultTlsCertificateInspector
import io.shaprint.client.core.network.DefaultUnicastProbe
import io.shaprint.client.core.network.mdns.DefaultMdnsScanner
import io.shaprint.client.core.raster.ImageRasterizer
import io.shaprint.client.core.raster.PdfRasterizer
import io.shaprint.client.core.security.EncryptedTrustStore
import io.shaprint.client.features.discovery.DiscoveryScreen
import io.shaprint.client.features.discovery.DiscoveryViewModel
import io.shaprint.client.features.print.PrintJobForegroundService
import io.shaprint.client.features.print.PrintPreviewScreen
import io.shaprint.client.features.print.PrintViewModel
import io.shaprint.client.features.printers.PrintersScreen
import io.shaprint.client.features.printers.PrintersViewModel
import io.shaprint.client.features.servers.ServersScreen
import io.shaprint.client.features.servers.ServersViewModel
import java.io.File
import java.io.FileOutputStream

@Composable
fun ShaPrintNavHost(
    navController: NavHostController = rememberNavController()
) {
    val context = LocalContext.current
    val appContext = context.applicationContext

    val trustStore = remember { EncryptedTrustStore(appContext) }
    val ippsClient = remember { DefaultIppsClient() }
    val tlsInspector = remember { DefaultTlsCertificateInspector() }
    val mdnsScanner = remember { DefaultMdnsScanner(appContext) }
    val unicastProbe = remember { DefaultUnicastProbe() }

    val discoveryViewModel = remember { DiscoveryViewModel(mdnsScanner, unicastProbe) }
    val serversViewModel = remember { ServersViewModel(trustStore, tlsInspector) }
    val printersViewModel = remember { PrintersViewModel(trustStore, ippsClient) }
    val printViewModel = remember { PrintViewModel(trustStore, ippsClient) }

    val discoveryState by discoveryViewModel.uiState.collectAsState()
    val serversState by serversViewModel.uiState.collectAsState()
    val printersState by printersViewModel.uiState.collectAsState()
    val printState by printViewModel.uiState.collectAsState()

    val documentPickerLauncher = rememberLauncherForActivityResult(
        contract = ActivityResultContracts.OpenDocument()
    ) { uri: Uri? ->
        if (uri != null) {
            var displayName = "Document"
            context.contentResolver.query(uri, null, null, null, null)?.use { cursor ->
                val nameIndex = cursor.getColumnIndex(OpenableColumns.DISPLAY_NAME)
                if (nameIndex >= 0 && cursor.moveToFirst()) {
                    displayName = cursor.getString(nameIndex) ?: "Document"
                }
            }

            // Detect page count if PDF
            var pageCount = 1
            val mimeType = context.contentResolver.getType(uri) ?: ""
            if (mimeType.contains("pdf", ignoreCase = true) || displayName.endsWith(".pdf", ignoreCase = true)) {
                try {
                    context.contentResolver.openFileDescriptor(uri, "r")?.use { pfd ->
                        PdfRenderer(pfd).use { renderer ->
                            pageCount = renderer.pageCount.coerceAtLeast(1)
                        }
                    }
                } catch (_: Exception) {
                    pageCount = 1
                }
            }

            printViewModel.selectDocument(displayName, uri.toString(), pageCount)
        }
    }

    NavHost(
        navController = navController,
        startDestination = Screen.Discovery.route
    ) {
        composable(Screen.Discovery.route) {
            DiscoveryScreen(
                uiState = discoveryState,
                onStartScan = { discoveryViewModel.startScan() },
                onOpenManualDialog = { discoveryViewModel.openManualServerDialog() },
                onDismissManualDialog = { discoveryViewModel.dismissManualServerDialog() },
                onUpdateManualHost = { discoveryViewModel.updateManualHost(it) },
                onUpdateManualPort = { discoveryViewModel.updateManualPort(it) },
                onSubmitManualServer = { discoveryViewModel.submitManualServer() },
                onNavigateToServers = {
                    serversViewModel.loadTrustedServers()
                    navController.navigate(Screen.Servers.route)
                },
                onNavigateToPrinters = {
                    printersViewModel.loadPrinters()
                    navController.navigate(Screen.Printers.route)
                },
                onServerClick = { nearbyServer ->
                    serversViewModel.reviewServer(nearbyServer)
                    navController.navigate(Screen.Servers.route)
                }
            )
        }
        composable(Screen.Servers.route) {
            ServersScreen(
                uiState = serversState,
                onApprovePendingServer = { serversViewModel.approvePendingServer() },
                onDismissReviewDialog = { serversViewModel.dismissReviewDialog() },
                onUpdateChannelInput = { serversViewModel.updateChannelInput(it) },
                onForgetServer = { serversViewModel.forgetServer(it) },
                onNavigateBack = { navController.popBackStack() },
                onNavigateToPrinters = {
                    printersViewModel.loadPrinters()
                    navController.navigate(Screen.Printers.route)
                }
            )
        }
        composable(Screen.Printers.route) {
            PrintersScreen(
                uiState = printersState,
                onRefresh = { printersViewModel.loadPrinters() },
                onNavigateBack = { navController.popBackStack() },
                onSelectPrinter = { printer ->
                    printersViewModel.selectPrinter(printer)
                    printViewModel.setPrinter(printer)
                    navController.navigate(Screen.PrintPreview.route)
                }
            )
        }
        composable(Screen.PrintPreview.route) {
            PrintPreviewScreen(
                uiState = printState,
                onPickDocument = {
                    documentPickerLauncher.launch(
                        arrayOf("application/pdf", "image/png", "image/jpeg", "image/webp")
                    )
                },
                onUpdateMedia = { printViewModel.updateMedia(it) },
                onUpdateColorMode = { printViewModel.updateColorMode(it) },
                onUpdateDuplexMode = { printViewModel.updateDuplexMode(it) },
                onUpdateOrientation = { printViewModel.updateOrientation(it) },
                onUpdateCopies = { printViewModel.updateCopies(it) },
                onUpdateFitToPage = { printViewModel.updateFitToPage(it) },
                onNavigateBack = { navController.popBackStack() },
                onSubmitPrint = {
                    val serviceIntent = Intent(context, PrintJobForegroundService::class.java).apply {
                        putExtra(PrintJobForegroundService.EXTRA_JOB_NAME, printState.documentName)
                        putExtra(
                            PrintJobForegroundService.EXTRA_PRINTER_NAME,
                            printState.selectedPrinter?.name ?: "Shared Printer"
                        )
                    }
                    try {
                        context.startService(serviceIntent)
                    } catch (_: Exception) {
                        // Ignore foreground start restriction in unit/preview context
                    }

                    printViewModel.submitPrintJob { outputStream ->
                        val uriStr = printState.documentUriString
                        if (uriStr != null) {
                            val uri = Uri.parse(uriStr)
                            val mime = context.contentResolver.getType(uri) ?: ""
                            if (mime.contains("pdf", ignoreCase = true) || printState.documentName.endsWith(".pdf", ignoreCase = true)) {
                                val tempFile = File.createTempFile("shaprint_inapp_", ".pdf", context.cacheDir)
                                try {
                                    context.contentResolver.openInputStream(uri)?.use { input ->
                                        FileOutputStream(tempFile).use { out -> input.copyTo(out) }
                                    }
                                    ParcelFileDescriptor.open(tempFile, ParcelFileDescriptor.MODE_READ_ONLY).use { pfd ->
                                        PdfRenderer(pfd).use { renderer ->
                                            PdfRasterizer.streamPdfToPwg(
                                                pdfRenderer = renderer,
                                                media = printState.selectedMedia,
                                                colorMode = printState.colorMode,
                                                out = outputStream
                                            )
                                        }
                                    }
                                } finally {
                                    tempFile.delete()
                                }
                            } else {
                                context.contentResolver.openInputStream(uri)?.use { input ->
                                    val bitmap = BitmapFactory.decodeStream(input)
                                    if (bitmap != null) {
                                        try {
                                            ImageRasterizer.streamImageToPwg(
                                                bitmap = bitmap,
                                                media = printState.selectedMedia,
                                                colorMode = printState.colorMode,
                                                fitToPage = printState.fitToPage,
                                                out = outputStream
                                            )
                                        } finally {
                                            bitmap.recycle()
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            )
        }
    }
}
