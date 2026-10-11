package io.shaprint.client.ui.navigation

import android.content.Intent
import android.graphics.BitmapFactory
import android.graphics.pdf.PdfRenderer
import android.net.Uri
import android.os.ParcelFileDescriptor
import android.provider.OpenableColumns
import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.foundation.layout.padding
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.Print
import androidx.compose.material.icons.filled.Security
import androidx.compose.material.icons.filled.Wifi
import androidx.compose.material3.Icon
import androidx.compose.material3.NavigationBar
import androidx.compose.material3.NavigationBarItem
import androidx.compose.material3.Scaffold
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.runtime.remember
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalContext
import androidx.navigation.NavGraph.Companion.findStartDestination
import androidx.navigation.NavHostController
import androidx.navigation.compose.NavHost
import androidx.navigation.compose.composable
import androidx.navigation.compose.currentBackStackEntryAsState
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

    val navBackStackEntry by navController.currentBackStackEntryAsState()
    val currentRoute = navBackStackEntry?.destination?.route

    val bottomNavItems = listOf(
        Triple(Screen.Printers, Icons.Default.Print, "Printers"),
        Triple(Screen.Discovery, Icons.Default.Wifi, "Discovery"),
        Triple(Screen.Servers, Icons.Default.Security, "Servers")
    )

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

    Scaffold(
        bottomBar = {
            if (currentRoute in listOf(Screen.Printers.route, Screen.Discovery.route, Screen.Servers.route)) {
                NavigationBar {
                    bottomNavItems.forEach { (screen, icon, label) ->
                        val isSelected = currentRoute == screen.route
                        NavigationBarItem(
                            icon = { Icon(icon, contentDescription = label) },
                            label = { Text(label) },
                            selected = isSelected,
                            onClick = {
                                if (currentRoute != screen.route) {
                                    if (screen == Screen.Printers) {
                                        printersViewModel.loadPrinters()
                                    } else if (screen == Screen.Servers) {
                                        serversViewModel.loadTrustedServers()
                                    }
                                    navController.navigate(screen.route) {
                                        popUpTo(navController.graph.findStartDestination().id) {
                                            saveState = true
                                        }
                                        launchSingleTop = true
                                        restoreState = true
                                    }
                                }
                            }
                        )
                    }
                }
            }
        }
    ) { innerPadding ->
        NavHost(
            navController = navController,
            startDestination = Screen.Printers.route,
            modifier = Modifier.padding(innerPadding)
        ) {
            composable(Screen.Printers.route) {
                PrintersScreen(
                    uiState = printersState,
                    onRefresh = { printersViewModel.loadPrinters() },
                    onNavigateToDiscovery = {
                        navController.navigate(Screen.Discovery.route) {
                            popUpTo(navController.graph.findStartDestination().id) {
                                saveState = true
                            }
                            launchSingleTop = true
                            restoreState = true
                        }
                    },
                    onSelectPrinter = { printer ->
                        printersViewModel.selectPrinter(printer)
                        printViewModel.setPrinter(printer)
                        navController.navigate(Screen.PrintPreview.route)
                    }
                )
            }
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
                        navController.navigate(Screen.Servers.route) {
                            popUpTo(navController.graph.findStartDestination().id) {
                                saveState = true
                            }
                            launchSingleTop = true
                            restoreState = true
                        }
                    },
                    onNavigateToPrinters = {
                        printersViewModel.loadPrinters()
                        navController.navigate(Screen.Printers.route) {
                            popUpTo(navController.graph.findStartDestination().id) {
                                saveState = true
                            }
                            launchSingleTop = true
                            restoreState = true
                        }
                    },
                    onServerClick = { nearbyServer ->
                        serversViewModel.reviewServer(nearbyServer)
                        navController.navigate(Screen.Servers.route) {
                            popUpTo(navController.graph.findStartDestination().id) {
                                saveState = true
                            }
                            launchSingleTop = true
                            restoreState = true
                        }
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
                        navController.navigate(Screen.Printers.route) {
                            popUpTo(navController.graph.findStartDestination().id) {
                                saveState = true
                            }
                            launchSingleTop = true
                            restoreState = true
                        }
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
}