package io.shaprint.client.features.print

import android.graphics.Bitmap
import androidx.compose.animation.AnimatedVisibility
import androidx.compose.foundation.BorderStroke
import androidx.compose.foundation.Image
import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.clickable
import androidx.compose.foundation.horizontalScroll
import androidx.compose.foundation.isSystemInDarkTheme
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.verticalScroll
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.filled.ArrowBack
import androidx.compose.material.icons.automirrored.filled.ArrowForward
import androidx.compose.material.icons.filled.Add
import androidx.compose.material.icons.filled.CheckCircle
import androidx.compose.material.icons.filled.Close
import androidx.compose.material.icons.filled.Description
import androidx.compose.material.icons.filled.Error
import androidx.compose.material.icons.filled.FileOpen
import androidx.compose.material.icons.filled.Print
import androidx.compose.material.icons.filled.Remove
import androidx.compose.material.icons.filled.ZoomIn
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.ColorFilter
import androidx.compose.ui.graphics.ColorMatrix
import androidx.compose.ui.graphics.asImageBitmap
import androidx.compose.ui.layout.ContentScale
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.compose.ui.window.Dialog
import androidx.compose.ui.window.DialogProperties
import io.shaprint.client.core.domain.model.DuplexMode
import io.shaprint.client.core.domain.model.PrintColorMode
import io.shaprint.client.core.domain.model.PrintJobState
import io.shaprint.client.core.domain.model.PrintOrientation
import io.shaprint.client.core.domain.model.RequestedMedia
import io.shaprint.client.ui.theme.*

@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun PrintPreviewScreen(
    uiState: PrintUiState = PrintUiState(),
    printerName: String = uiState.selectedPrinter?.name ?: "Shared Printer",
    onPickDocument: () -> Unit = {},
    onUpdateMedia: (RequestedMedia) -> Unit = {},
    onUpdateColorMode: (PrintColorMode) -> Unit = {},
    onUpdateDuplexMode: (DuplexMode) -> Unit = {},
    onUpdateOrientation: (PrintOrientation) -> Unit = {},
    onUpdateCopies: (Int) -> Unit = {},
    onUpdateFitToPage: (Boolean) -> Unit = {},
    onNavigateBack: () -> Unit = {},
    onSubmitPrint: () -> Unit = {}
) {
    val supportedMedia = uiState.selectedPrinter?.capabilities?.supportedMedia
        ?: RequestedMedia.entries
    val supportedColors = uiState.selectedPrinter?.capabilities?.supportedColorModes
        ?: PrintColorMode.entries
    val supportedDuplex = uiState.selectedPrinter?.capabilities?.supportedDuplexModes
        ?: DuplexMode.entries

    val isWorking = uiState.jobProgress.state == PrintJobState.RASTERIZING ||
        uiState.jobProgress.state == PrintJobState.STREAMING

    var previewPageIndex by remember(uiState.documentUriString, uiState.documentRevision) { mutableIntStateOf(0) }
    var showFullScreenPreview by remember { mutableStateOf(false) }

    val context = LocalContext.current
    val previewBitmap by produceState<Bitmap?>(
        initialValue = DocumentPreviewHelper.getCachedBitmap(
            documentUriString = uiState.documentUriString,
            documentRevision = uiState.documentRevision,
            pageIndex = previewPageIndex,
            media = uiState.selectedMedia,
            orientation = uiState.orientation,
            fitToPage = uiState.fitToPage
        ),
        uiState.documentUriString,
        uiState.documentRevision,
        previewPageIndex,
        uiState.selectedMedia,
        uiState.orientation,
        uiState.fitToPage
    ) {
        val cached = DocumentPreviewHelper.getCachedBitmap(
            documentUriString = uiState.documentUriString,
            documentRevision = uiState.documentRevision,
            pageIndex = previewPageIndex,
            media = uiState.selectedMedia,
            orientation = uiState.orientation,
            fitToPage = uiState.fitToPage
        )
        if (cached != null) {
            value = cached
        }
        val loaded = DocumentPreviewHelper.loadPreviewBitmap(
            context = context,
            documentUriString = uiState.documentUriString,
            documentName = uiState.documentName,
            documentRevision = uiState.documentRevision,
            pageIndex = previewPageIndex,
            media = uiState.selectedMedia,
            colorMode = uiState.colorMode,
            orientation = uiState.orientation,
            fitToPage = uiState.fitToPage,
            printerName = printerName,
            serverCanonicalAddress = uiState.selectedPrinter?.serverCanonicalAddress ?: "LAN Server",
            targetWidth = 600
        )
        if (loaded != null) {
            value = loaded
        }
    }

    val currentBitmap = previewBitmap
    if (showFullScreenPreview && currentBitmap != null) {
        FullScreenDocumentDialog(
            bitmap = currentBitmap,
            documentName = uiState.documentName,
            pageIndex = previewPageIndex,
            totalPages = uiState.totalPages,
            media = uiState.selectedMedia,
            colorMode = uiState.colorMode,
            orientation = uiState.orientation,
            onDismiss = { showFullScreenPreview = false }
        )
    }

    Scaffold(
        topBar = {
            TopAppBar(
                title = {
                    Column {
                        Text(
                            text = "Print Document",
                            style = MaterialTheme.typography.titleLarge
                        )
                        Text(
                            text = printerName,
                            style = MaterialTheme.typography.labelSmall,
                            color = MaterialTheme.colorScheme.onSurfaceVariant,
                            maxLines = 1,
                            overflow = TextOverflow.Ellipsis
                        )
                    }
                },
                navigationIcon = {
                    IconButton(onClick = onNavigateBack) {
                        Icon(
                            imageVector = Icons.AutoMirrored.Filled.ArrowBack,
                            contentDescription = "Navigate back"
                        )
                    }
                },
                actions = {
                    FilledTonalButton(
                        onClick = onPickDocument,
                        shape = RoundedCornerShape(10.dp),
                        contentPadding = PaddingValues(horizontal = 12.dp, vertical = 6.dp)
                    ) {
                        Icon(
                            imageVector = Icons.Default.FileOpen,
                            contentDescription = null,
                            modifier = Modifier.size(16.dp)
                        )
                        Spacer(modifier = Modifier.width(4.dp))
                        Text("Pick File")
                    }
                }
            )
        },
        bottomBar = {
            Surface(
                tonalElevation = 6.dp,
                shadowElevation = 8.dp,
                modifier = Modifier.fillMaxWidth()
            ) {
                Box(modifier = Modifier.padding(16.dp)) {
                    Button(
                        onClick = onSubmitPrint,
                        enabled = !isWorking,
                        shape = RoundedCornerShape(12.dp),
                        modifier = Modifier
                            .fillMaxWidth()
                            .height(52.dp)
                    ) {
                        if (isWorking) {
                            CircularProgressIndicator(
                                modifier = Modifier.size(20.dp),
                                color = MaterialTheme.colorScheme.onPrimary,
                                strokeWidth = 2.dp
                            )
                            Spacer(modifier = Modifier.width(10.dp))
                            Text("Sending to $printerName...")
                        } else {
                            Icon(
                                imageVector = Icons.Default.Print,
                                contentDescription = null,
                                modifier = Modifier.size(20.dp)
                            )
                            Spacer(modifier = Modifier.width(8.dp))
                            Text(
                                text = if (uiState.documentUriString == null) {
                                    "Print Test Page to $printerName"
                                } else {
                                    "Print to $printerName"
                                },
                                style = MaterialTheme.typography.titleSmall,
                                fontWeight = FontWeight.Bold
                            )
                        }
                    }
                }
            }
        }
    ) { padding ->
        Column(
            modifier = Modifier
                .fillMaxSize()
                .padding(padding)
                .verticalScroll(rememberScrollState())
                .padding(horizontal = 16.dp, vertical = 12.dp)
        ) {
            // Visual Document Paper Sheet Preview Card
            DocumentPaperPreviewCard(
                documentName = uiState.documentName,
                isTestPage = uiState.documentUriString == null,
                totalPages = uiState.totalPages,
                currentPage = previewPageIndex,
                media = uiState.selectedMedia,
                colorMode = uiState.colorMode,
                orientation = uiState.orientation,
                previewBitmap = previewBitmap,
                onChangeFile = onPickDocument,
                onPreviousPage = {
                    if (previewPageIndex > 0) previewPageIndex--
                },
                onNextPage = {
                    if (previewPageIndex < uiState.totalPages - 1) previewPageIndex++
                },
                onCardClick = {
                    if (previewBitmap != null) {
                        showFullScreenPreview = true
                    } else {
                        onPickDocument()
                    }
                }
            )

            Spacer(modifier = Modifier.height(16.dp))

            // Job Progress / Status Feedback
            AnimatedVisibility(visible = isWorking) {
                Card(
                    colors = CardDefaults.cardColors(
                        containerColor = MaterialTheme.colorScheme.secondaryContainer
                    ),
                    shape = RoundedCornerShape(14.dp),
                    modifier = Modifier
                        .fillMaxWidth()
                        .padding(bottom = 16.dp)
                ) {
                    Column(modifier = Modifier.padding(14.dp)) {
                        Row(
                            verticalAlignment = Alignment.CenterVertically,
                            modifier = Modifier.fillMaxWidth()
                        ) {
                            CircularProgressIndicator(
                                modifier = Modifier.size(20.dp),
                                strokeWidth = 2.5.dp
                            )
                            Spacer(modifier = Modifier.width(10.dp))
                            Text(
                                text = if (uiState.jobProgress.state == PrintJobState.RASTERIZING) {
                                    "Rasterizing page ${uiState.jobProgress.currentPage} of ${uiState.jobProgress.totalPages}..."
                                } else {
                                    "Streaming PWG-raster to printer..."
                                },
                                style = MaterialTheme.typography.titleSmall,
                                fontWeight = FontWeight.SemiBold
                            )
                        }
                        Spacer(modifier = Modifier.height(8.dp))
                        LinearProgressIndicator(
                            modifier = Modifier
                                .fillMaxWidth()
                                .clip(RoundedCornerShape(4.dp))
                        )
                    }
                }
            }

            // High-Contrast Success Card (Accessible in both Light & Dark Theme)
            AnimatedVisibility(visible = uiState.jobProgress.state == PrintJobState.COMPLETED) {
                val isDark = isSystemInDarkTheme()
                Card(
                    colors = CardDefaults.cardColors(
                        containerColor = if (isDark) OnlineGreenContainerDark else OnlineGreenContainer
                    ),
                    border = BorderStroke(1.5.dp, if (isDark) OnlineGreenDark else OnlineGreen),
                    shape = RoundedCornerShape(14.dp),
                    modifier = Modifier
                        .fillMaxWidth()
                        .padding(bottom = 16.dp)
                ) {
                    Row(
                        modifier = Modifier.padding(16.dp),
                        verticalAlignment = Alignment.CenterVertically
                    ) {
                        Box(
                            modifier = Modifier
                                .size(36.dp)
                                .clip(CircleShape)
                                .background(if (isDark) OnlineGreenDark.copy(alpha = 0.2f) else OnlineGreen.copy(alpha = 0.15f)),
                            contentAlignment = Alignment.Center
                        ) {
                            Icon(
                                imageVector = Icons.Default.CheckCircle,
                                contentDescription = null,
                                tint = if (isDark) OnlineGreenDark else OnlineGreen,
                                modifier = Modifier.size(22.dp)
                            )
                        }
                        Spacer(modifier = Modifier.width(12.dp))
                        Column(modifier = Modifier.weight(1f)) {
                            Text(
                                text = "Print Job Accepted",
                                style = MaterialTheme.typography.titleMedium,
                                fontWeight = FontWeight.Bold,
                                color = if (isDark) OnlineGreenDark else OnlineGreen
                            )
                            Spacer(modifier = Modifier.height(2.dp))
                            Text(
                                text = "Job successfully transmitted and accepted by $printerName!",
                                color = if (isDark) OnOnlineGreenContainerDark else OnOnlineGreenContainer,
                                style = MaterialTheme.typography.bodyMedium,
                                fontWeight = FontWeight.Medium
                            )
                        }
                    }
                }
            }

            // High-Contrast Error Card (Accessible in both Light & Dark Theme)
            AnimatedVisibility(visible = uiState.jobProgress.state == PrintJobState.FAILED) {
                val isDark = isSystemInDarkTheme()
                Card(
                    colors = CardDefaults.cardColors(
                        containerColor = if (isDark) ErrorRoseContainerDark else ErrorRoseContainer
                    ),
                    border = BorderStroke(1.5.dp, if (isDark) ErrorRoseDark else ErrorRose),
                    shape = RoundedCornerShape(14.dp),
                    modifier = Modifier
                        .fillMaxWidth()
                        .padding(bottom = 16.dp)
                ) {
                    Row(
                        modifier = Modifier.padding(16.dp),
                        verticalAlignment = Alignment.Top
                    ) {
                        Box(
                            modifier = Modifier
                                .size(36.dp)
                                .clip(CircleShape)
                                .background(if (isDark) ErrorRoseDark.copy(alpha = 0.2f) else ErrorRose.copy(alpha = 0.15f)),
                            contentAlignment = Alignment.Center
                        ) {
                            Icon(
                                imageVector = Icons.Default.Error,
                                contentDescription = null,
                                tint = if (isDark) ErrorRoseDark else ErrorRose,
                                modifier = Modifier.size(22.dp)
                            )
                        }
                        Spacer(modifier = Modifier.width(12.dp))
                        Column(modifier = Modifier.weight(1f)) {
                            Text(
                                text = "Print Job Failed",
                                style = MaterialTheme.typography.titleMedium,
                                fontWeight = FontWeight.Bold,
                                color = if (isDark) ErrorRoseDark else ErrorRose
                            )
                            Spacer(modifier = Modifier.height(4.dp))
                            Text(
                                text = uiState.jobProgress.errorMessage ?: "The print job was rejected or could not be delivered.",
                                color = if (isDark) OnErrorRoseContainerDark else OnErrorRoseContainer,
                                style = MaterialTheme.typography.bodyMedium,
                                fontWeight = FontWeight.Medium
                            )
                        }
                    }
                }
            }

            // Print Settings Form Section
            Text(
                text = "Print Settings",
                style = MaterialTheme.typography.titleMedium,
                fontWeight = FontWeight.Bold
            )

            Spacer(modifier = Modifier.height(12.dp))

            // 1. Paper Format (Media)
            Card(
                shape = RoundedCornerShape(14.dp),
                colors = CardDefaults.cardColors(containerColor = MaterialTheme.colorScheme.surface),
                elevation = CardDefaults.cardElevation(defaultElevation = 1.dp),
                modifier = Modifier.fillMaxWidth()
            ) {
                Column(modifier = Modifier.padding(14.dp)) {
                    Text(
                        text = "Paper Format",
                        style = MaterialTheme.typography.titleSmall,
                        fontWeight = FontWeight.SemiBold
                    )
                    Spacer(modifier = Modifier.height(8.dp))
                    Row(
                        horizontalArrangement = Arrangement.spacedBy(8.dp),
                        modifier = Modifier
                            .fillMaxWidth()
                            .horizontalScroll(rememberScrollState())
                    ) {
                        supportedMedia.forEach { media ->
                            val isSelected = media == uiState.selectedMedia
                            FilterChip(
                                selected = isSelected,
                                onClick = { onUpdateMedia(media) },
                                label = { Text(media.displayName) },
                                shape = RoundedCornerShape(10.dp)
                            )
                        }
                    }
                }
            }

            Spacer(modifier = Modifier.height(12.dp))

            // 2. Color Mode
            Card(
                shape = RoundedCornerShape(14.dp),
                colors = CardDefaults.cardColors(containerColor = MaterialTheme.colorScheme.surface),
                elevation = CardDefaults.cardElevation(defaultElevation = 1.dp),
                modifier = Modifier.fillMaxWidth()
            ) {
                Column(modifier = Modifier.padding(14.dp)) {
                    Text(
                        text = "Color Mode",
                        style = MaterialTheme.typography.titleSmall,
                        fontWeight = FontWeight.SemiBold
                    )
                    Spacer(modifier = Modifier.height(8.dp))
                    Row(
                        horizontalArrangement = Arrangement.spacedBy(8.dp),
                        modifier = Modifier.fillMaxWidth()
                    ) {
                        supportedColors.forEach { color ->
                            val isSelected = color == uiState.colorMode
                            FilterChip(
                                selected = isSelected,
                                onClick = { onUpdateColorMode(color) },
                                label = { Text(color.displayName) },
                                shape = RoundedCornerShape(10.dp),
                                modifier = Modifier.weight(1f)
                            )
                        }
                    }
                }
            }

            Spacer(modifier = Modifier.height(12.dp))

            // 3. Duplex / Sides
            Card(
                shape = RoundedCornerShape(14.dp),
                colors = CardDefaults.cardColors(containerColor = MaterialTheme.colorScheme.surface),
                elevation = CardDefaults.cardElevation(defaultElevation = 1.dp),
                modifier = Modifier.fillMaxWidth()
            ) {
                Column(modifier = Modifier.padding(14.dp)) {
                    Text(
                        text = "Duplex Printing (Sides)",
                        style = MaterialTheme.typography.titleSmall,
                        fontWeight = FontWeight.SemiBold
                    )
                    Spacer(modifier = Modifier.height(8.dp))
                    Column(verticalArrangement = Arrangement.spacedBy(6.dp)) {
                        supportedDuplex.forEach { duplex ->
                            val isSelected = duplex == uiState.duplexMode
                            FilterChip(
                                selected = isSelected,
                                onClick = { onUpdateDuplexMode(duplex) },
                                label = { Text(duplex.displayName) },
                                shape = RoundedCornerShape(10.dp),
                                modifier = Modifier.fillMaxWidth()
                            )
                        }
                    }
                }
            }

            Spacer(modifier = Modifier.height(12.dp))

            // 4. Orientation
            Card(
                shape = RoundedCornerShape(14.dp),
                colors = CardDefaults.cardColors(containerColor = MaterialTheme.colorScheme.surface),
                elevation = CardDefaults.cardElevation(defaultElevation = 1.dp),
                modifier = Modifier.fillMaxWidth()
            ) {
                Column(modifier = Modifier.padding(14.dp)) {
                    Text(
                        text = "Page Orientation",
                        style = MaterialTheme.typography.titleSmall,
                        fontWeight = FontWeight.SemiBold
                    )
                    Spacer(modifier = Modifier.height(8.dp))
                    Row(
                        horizontalArrangement = Arrangement.spacedBy(8.dp),
                        modifier = Modifier.fillMaxWidth()
                    ) {
                        PrintOrientation.entries.forEach { orientation ->
                            val isSelected = orientation == uiState.orientation
                            FilterChip(
                                selected = isSelected,
                                onClick = { onUpdateOrientation(orientation) },
                                label = { Text(orientation.displayName) },
                                shape = RoundedCornerShape(10.dp),
                                modifier = Modifier.weight(1f)
                            )
                        }
                    }
                }
            }

            Spacer(modifier = Modifier.height(12.dp))

            // 5. Copies and Fit-To-Page Row
            Card(
                shape = RoundedCornerShape(14.dp),
                colors = CardDefaults.cardColors(containerColor = MaterialTheme.colorScheme.surface),
                elevation = CardDefaults.cardElevation(defaultElevation = 1.dp),
                modifier = Modifier.fillMaxWidth()
            ) {
                Column(modifier = Modifier.padding(14.dp)) {
                    // Copies Stepper
                    Row(
                        modifier = Modifier.fillMaxWidth(),
                        horizontalArrangement = Arrangement.SpaceBetween,
                        verticalAlignment = Alignment.CenterVertically
                    ) {
                        Column {
                            Text(
                                text = "Copies",
                                style = MaterialTheme.typography.titleSmall,
                                fontWeight = FontWeight.SemiBold
                            )
                            Text(
                                text = "Number of duplicates to print",
                                style = MaterialTheme.typography.bodySmall,
                                color = MaterialTheme.colorScheme.onSurfaceVariant
                            )
                        }

                        Row(
                            verticalAlignment = Alignment.CenterVertically,
                            modifier = Modifier
                                .border(
                                    1.dp,
                                    MaterialTheme.colorScheme.outlineVariant,
                                    RoundedCornerShape(10.dp)
                                )
                                .padding(horizontal = 4.dp, vertical = 2.dp)
                        ) {
                            IconButton(
                                onClick = { onUpdateCopies((uiState.copies - 1).coerceAtLeast(1)) },
                                enabled = uiState.copies > 1,
                                modifier = Modifier.size(36.dp)
                            ) {
                                Icon(Icons.Default.Remove, contentDescription = "Decrease copies", modifier = Modifier.size(18.dp))
                            }

                            Text(
                                text = "${uiState.copies}",
                                style = MaterialTheme.typography.titleMedium,
                                fontWeight = FontWeight.Bold,
                                modifier = Modifier.padding(horizontal = 12.dp)
                            )

                            IconButton(
                                onClick = { onUpdateCopies((uiState.copies + 1).coerceAtMost(999)) },
                                modifier = Modifier.size(36.dp)
                            ) {
                                Icon(Icons.Default.Add, contentDescription = "Increase copies", modifier = Modifier.size(18.dp))
                            }
                        }
                    }

                    Spacer(modifier = Modifier.height(12.dp))
                    HorizontalDivider(color = MaterialTheme.colorScheme.outlineVariant)
                    Spacer(modifier = Modifier.height(12.dp))

                    // Fit to printable page
                    Row(
                        modifier = Modifier.fillMaxWidth(),
                        horizontalArrangement = Arrangement.SpaceBetween,
                        verticalAlignment = Alignment.CenterVertically
                    ) {
                        Column {
                            Text(
                                text = "Fit to Printable Area",
                                style = MaterialTheme.typography.titleSmall,
                                fontWeight = FontWeight.SemiBold
                            )
                            Text(
                                text = "Scale image or document page to fit paper margins",
                                style = MaterialTheme.typography.bodySmall,
                                color = MaterialTheme.colorScheme.onSurfaceVariant
                            )
                        }

                        Switch(
                            checked = uiState.fitToPage,
                            onCheckedChange = onUpdateFitToPage
                        )
                    }
                }
            }

            Spacer(modifier = Modifier.height(24.dp))
        }
    }
}

/**
 * Visual Document Paper Preview Card displaying real rendered page bitmaps with
 * responsive orientation aspect ratio, greyscale filter, and page navigation controls.
 */
@Composable
private fun DocumentPaperPreviewCard(
    documentName: String,
    isTestPage: Boolean,
    totalPages: Int,
    currentPage: Int,
    media: RequestedMedia,
    colorMode: PrintColorMode,
    orientation: PrintOrientation,
    previewBitmap: Bitmap?,
    onChangeFile: () -> Unit,
    onPreviousPage: () -> Unit,
    onNextPage: () -> Unit,
    onCardClick: () -> Unit
) {
    Card(
        shape = RoundedCornerShape(18.dp),
        colors = CardDefaults.cardColors(
            containerColor = MaterialTheme.colorScheme.surfaceVariant
        ),
        modifier = Modifier.fillMaxWidth()
    ) {
        Column(
            horizontalAlignment = Alignment.CenterHorizontally,
            modifier = Modifier.padding(16.dp)
        ) {
            // Header: Document title & badge
            Row(
                modifier = Modifier.fillMaxWidth(),
                horizontalArrangement = Arrangement.SpaceBetween,
                verticalAlignment = Alignment.CenterVertically
            ) {
                Column(modifier = Modifier.weight(1f)) {
                    Text(
                        text = if (isTestPage) "Official ShaPrint Test Page" else documentName,
                        style = MaterialTheme.typography.titleMedium,
                        fontWeight = FontWeight.Bold,
                        maxLines = 1,
                        overflow = TextOverflow.Ellipsis
                    )
                    Text(
                        text = if (isTestPage) "Ready to verify printer connection" else "$totalPages page(s) • ${media.displayName}",
                        style = MaterialTheme.typography.bodySmall,
                        color = MaterialTheme.colorScheme.onSurfaceVariant
                    )
                }

                Surface(
                    shape = RoundedCornerShape(8.dp),
                    color = if (isTestPage) MaterialTheme.colorScheme.primaryContainer else MaterialTheme.colorScheme.secondaryContainer
                ) {
                    Text(
                        text = if (isTestPage) "Test Page" else "${media.displayName.split(" ").firstOrNull() ?: "A4"}",
                        style = MaterialTheme.typography.labelSmall,
                        fontWeight = FontWeight.Bold,
                        color = if (isTestPage) MaterialTheme.colorScheme.onPrimaryContainer else MaterialTheme.colorScheme.onSecondaryContainer,
                        modifier = Modifier.padding(horizontal = 8.dp, vertical = 4.dp)
                    )
                }
            }

            Spacer(modifier = Modifier.height(16.dp))

            // True 1:1 Physical Paper Aspect Ratio Sheet Box
            val isLandscape = orientation == PrintOrientation.LANDSCAPE
            val paperWidthMm = if (isLandscape) media.heightMm else media.widthMm
            val paperHeightMm = if (isLandscape) media.widthMm else media.heightMm
            val paperRatio = paperWidthMm / paperHeightMm

            val (sheetWidth, sheetHeight) = if (isLandscape) {
                val w = 270.dp
                val h = 270.dp / paperRatio
                w to h
            } else {
                val h = 280.dp
                val w = 280.dp * paperRatio
                w to h
            }

            Surface(
                color = Color.White,
                shape = RoundedCornerShape(4.dp),
                shadowElevation = 8.dp,
                border = BorderStroke(1.dp, Color(0xFFD1D5DB)),
                modifier = Modifier
                    .width(sheetWidth)
                    .height(sheetHeight)
                    .clickable(onClick = onCardClick)
            ) {
                Box(
                    modifier = Modifier.fillMaxSize(),
                    contentAlignment = Alignment.Center
                ) {
                    if (previewBitmap != null) {
                        val colorFilter = if (colorMode == PrintColorMode.MONOCHROME) {
                            ColorFilter.colorMatrix(ColorMatrix().apply { setToSaturation(0f) })
                        } else null

                        Image(
                            bitmap = previewBitmap.asImageBitmap(),
                            contentDescription = "Document Page Preview",
                            modifier = Modifier.fillMaxSize(),
                            contentScale = ContentScale.FillBounds,
                            colorFilter = colorFilter
                        )

                        // Enlarge hint badge on bottom right of paper
                        Surface(
                            color = Color.Black.copy(alpha = 0.55f),
                            shape = CircleShape,
                            modifier = Modifier
                                .align(Alignment.BottomEnd)
                                .padding(8.dp)
                                .size(28.dp)
                        ) {
                            Box(contentAlignment = Alignment.Center) {
                                Icon(
                                    imageVector = Icons.Default.ZoomIn,
                                    contentDescription = "Zoom",
                                    tint = Color.White,
                                    modifier = Modifier.size(16.dp)
                                )
                            }
                        }
                    } else {
                        // Loading / Placeholder
                        Column(
                            horizontalAlignment = Alignment.CenterHorizontally,
                            verticalArrangement = Arrangement.Center,
                            modifier = Modifier.padding(16.dp)
                        ) {
                            CircularProgressIndicator(
                                modifier = Modifier.size(28.dp),
                                strokeWidth = 2.5.dp
                            )
                            Spacer(modifier = Modifier.height(8.dp))
                            Text(
                                text = "Rendering preview...",
                                style = MaterialTheme.typography.labelSmall,
                                color = Color.Gray
                            )
                        }
                    }
                }
            }

            Spacer(modifier = Modifier.height(12.dp))

            // Multi-page navigation stepper (if document has > 1 page)
            if (totalPages > 1) {
                Row(
                    verticalAlignment = Alignment.CenterVertically,
                    horizontalArrangement = Arrangement.Center,
                    modifier = Modifier.fillMaxWidth()
                ) {
                    IconButton(
                        onClick = onPreviousPage,
                        enabled = currentPage > 0,
                        modifier = Modifier.size(32.dp)
                    ) {
                        Icon(
                            imageVector = Icons.AutoMirrored.Filled.ArrowBack,
                            contentDescription = "Previous page",
                            modifier = Modifier.size(18.dp)
                        )
                    }

                    Text(
                        text = "Page ${currentPage + 1} of $totalPages",
                        style = MaterialTheme.typography.labelMedium,
                        fontWeight = FontWeight.SemiBold,
                        modifier = Modifier.padding(horizontal = 12.dp)
                    )

                    IconButton(
                        onClick = onNextPage,
                        enabled = currentPage < totalPages - 1,
                        modifier = Modifier.size(32.dp)
                    ) {
                        Icon(
                            imageVector = Icons.AutoMirrored.Filled.ArrowForward,
                            contentDescription = "Next page",
                            modifier = Modifier.size(18.dp)
                        )
                    }
                }
                Spacer(modifier = Modifier.height(8.dp))
            }

            // Document info chips row & change file button
            Row(
                modifier = Modifier.fillMaxWidth(),
                horizontalArrangement = Arrangement.SpaceBetween,
                verticalAlignment = Alignment.CenterVertically
            ) {
                Surface(
                    shape = RoundedCornerShape(8.dp),
                    color = MaterialTheme.colorScheme.surface
                ) {
                    Text(
                        text = "${media.displayName.split(" ").firstOrNull() ?: "A4"} • ${colorMode.displayName} • ${orientation.displayName}",
                        style = MaterialTheme.typography.labelSmall,
                        modifier = Modifier.padding(horizontal = 10.dp, vertical = 4.dp)
                    )
                }

                TextButton(
                    onClick = onChangeFile,
                    contentPadding = PaddingValues(horizontal = 8.dp, vertical = 2.dp)
                ) {
                    Icon(
                        imageVector = Icons.Default.FileOpen,
                        contentDescription = null,
                        modifier = Modifier.size(16.dp)
                    )
                    Spacer(modifier = Modifier.width(4.dp))
                    Text(
                        text = if (isTestPage) "Choose Custom File" else "Change File",
                        style = MaterialTheme.typography.labelMedium,
                        fontWeight = FontWeight.SemiBold
                    )
                }
            }
        }
    }
}

/**
 * Fullscreen modal dialog allowing users to zoom and inspect the complete high-resolution
 * rendered document page before executing print.
 */
@Composable
private fun FullScreenDocumentDialog(
    bitmap: Bitmap,
    documentName: String,
    pageIndex: Int,
    totalPages: Int,
    media: RequestedMedia,
    colorMode: PrintColorMode,
    orientation: PrintOrientation,
    onDismiss: () -> Unit
) {
    Dialog(
        onDismissRequest = onDismiss,
        properties = DialogProperties(usePlatformDefaultWidth = false)
    ) {
        Surface(
            color = MaterialTheme.colorScheme.background,
            modifier = Modifier.fillMaxSize()
        ) {
            Column(
                modifier = Modifier
                    .fillMaxSize()
                    .padding(16.dp)
            ) {
                // Dialog Top Bar
                Row(
                    modifier = Modifier.fillMaxWidth(),
                    horizontalArrangement = Arrangement.SpaceBetween,
                    verticalAlignment = Alignment.CenterVertically
                ) {
                    Column(modifier = Modifier.weight(1f)) {
                        Text(
                            text = documentName,
                            style = MaterialTheme.typography.titleMedium,
                            fontWeight = FontWeight.Bold,
                            maxLines = 1,
                            overflow = TextOverflow.Ellipsis
                        )
                        Text(
                            text = if (totalPages > 1) "Page ${pageIndex + 1} of $totalPages • ${media.displayName} • ${colorMode.displayName}" else "${media.displayName} • ${colorMode.displayName} • ${orientation.displayName}",
                            style = MaterialTheme.typography.bodySmall,
                            color = MaterialTheme.colorScheme.onSurfaceVariant
                        )
                    }

                    IconButton(onClick = onDismiss) {
                        Icon(
                            imageVector = Icons.Default.Close,
                            contentDescription = "Close preview"
                        )
                    }
                }

                Spacer(modifier = Modifier.height(12.dp))

                // Large 1:1 Document Sheet Container
                Box(
                    modifier = Modifier
                        .weight(1f)
                        .fillMaxWidth()
                        .clip(RoundedCornerShape(12.dp))
                        .background(MaterialTheme.colorScheme.surfaceVariant)
                        .padding(12.dp),
                    contentAlignment = Alignment.Center
                ) {
                    val isLandscape = orientation == PrintOrientation.LANDSCAPE
                    val paperWidthMm = if (isLandscape) media.heightMm else media.widthMm
                    val paperHeightMm = if (isLandscape) media.widthMm else media.heightMm
                    val paperRatio = paperWidthMm / paperHeightMm

                    BoxWithConstraints(
                        modifier = Modifier.fillMaxSize(),
                        contentAlignment = Alignment.Center
                    ) {
                        val containerRatio = maxWidth / maxHeight
                        val (sheetWidth, sheetHeight) = if (paperRatio > containerRatio) {
                            maxWidth to (maxWidth / paperRatio)
                        } else {
                            (maxHeight * paperRatio) to maxHeight
                        }

                        Surface(
                            color = Color.White,
                            shape = RoundedCornerShape(4.dp),
                            shadowElevation = 8.dp,
                            border = BorderStroke(1.dp, Color(0xFFE5E7EB)),
                            modifier = Modifier
                                .width(sheetWidth)
                                .height(sheetHeight)
                        ) {
                            val colorFilter = if (colorMode == PrintColorMode.MONOCHROME) {
                                ColorFilter.colorMatrix(ColorMatrix().apply { setToSaturation(0f) })
                            } else null

                            Image(
                                bitmap = bitmap.asImageBitmap(),
                                contentDescription = "Full Document Preview",
                                modifier = Modifier.fillMaxSize(),
                                contentScale = ContentScale.FillBounds,
                                colorFilter = colorFilter
                            )
                        }
                    }
                }

                Spacer(modifier = Modifier.height(16.dp))

                Button(
                    onClick = onDismiss,
                    shape = RoundedCornerShape(12.dp),
                    modifier = Modifier.fillMaxWidth()
                ) {
                    Text("Close Preview")
                }
            }
        }
    }
}
