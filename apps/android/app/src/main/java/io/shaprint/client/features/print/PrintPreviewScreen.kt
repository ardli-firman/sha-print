package io.shaprint.client.features.print

import androidx.compose.foundation.background
import androidx.compose.foundation.layout.*
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.unit.dp
import io.shaprint.client.core.domain.model.DuplexMode
import io.shaprint.client.core.domain.model.PrintColorMode
import io.shaprint.client.core.domain.model.PrintJobState
import io.shaprint.client.core.domain.model.PrintOrientation
import io.shaprint.client.core.domain.model.RequestedMedia

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

    Scaffold(
        topBar = {
            TopAppBar(
                title = { Text("Print — $printerName") },
                navigationIcon = {
                    TextButton(onClick = onNavigateBack) {
                        Text("Back")
                    }
                },
                actions = {
                    TextButton(onClick = onPickDocument) {
                        Text("Pick File")
                    }
                }
            )
        }
    ) { padding ->
        Column(
            modifier = Modifier
                .fillMaxSize()
                .padding(padding)
                .padding(16.dp)
        ) {
            // Visual Preview Box
            Box(
                modifier = Modifier
                    .fillMaxWidth()
                    .height(180.dp)
                    .background(
                        color = MaterialTheme.colorScheme.surfaceVariant,
                        shape = MaterialTheme.shapes.medium
                    ),
                contentAlignment = Alignment.Center
            ) {
                Column(horizontalAlignment = Alignment.CenterHorizontally) {
                    Text(
                        text = uiState.documentName,
                        style = MaterialTheme.typography.titleMedium
                    )
                    Text(
                        text = "${uiState.totalPages} page(s) • 300 DPI PWG Raster (ADR 0010)",
                        style = MaterialTheme.typography.bodySmall,
                        color = MaterialTheme.colorScheme.outline
                    )
                    Spacer(modifier = Modifier.height(4.dp))
                    Text(
                        text = "${uiState.selectedMedia.displayName} • ${uiState.colorMode.displayName} • ${uiState.orientation.displayName}",
                        style = MaterialTheme.typography.bodySmall
                    )
                }
            }

            Spacer(modifier = Modifier.height(16.dp))

            // Job Progress or Error Status
            when (uiState.jobProgress.state) {
                PrintJobState.RASTERIZING, PrintJobState.STREAMING -> {
                    Card(
                        colors = CardDefaults.cardColors(
                            containerColor = MaterialTheme.colorScheme.secondaryContainer
                        ),
                        modifier = Modifier.fillMaxWidth()
                    ) {
                        Column(modifier = Modifier.padding(12.dp)) {
                            Text(
                                text = if (uiState.jobProgress.state == PrintJobState.RASTERIZING) {
                                    "Rasterizing page ${uiState.jobProgress.currentPage} of ${uiState.jobProgress.totalPages}..."
                                } else {
                                    "Streaming PWG-raster to $printerName..."
                                },
                                style = MaterialTheme.typography.bodyMedium
                            )
                            Spacer(modifier = Modifier.height(6.dp))
                            LinearProgressIndicator(modifier = Modifier.fillMaxWidth())
                        }
                    }
                    Spacer(modifier = Modifier.height(12.dp))
                }
                PrintJobState.COMPLETED -> {
                    Card(
                        colors = CardDefaults.cardColors(
                            containerColor = MaterialTheme.colorScheme.primaryContainer
                        ),
                        modifier = Modifier.fillMaxWidth()
                    ) {
                        Text(
                            text = "Print job submitted and accepted by $printerName!",
                            color = MaterialTheme.colorScheme.onPrimaryContainer,
                            style = MaterialTheme.typography.bodyMedium,
                            modifier = Modifier.padding(12.dp)
                        )
                    }
                    Spacer(modifier = Modifier.height(12.dp))
                }
                PrintJobState.FAILED -> {
                    Card(
                        colors = CardDefaults.cardColors(
                            containerColor = MaterialTheme.colorScheme.errorContainer
                        ),
                        modifier = Modifier.fillMaxWidth()
                    ) {
                        Text(
                            text = uiState.jobProgress.errorMessage ?: "Print job failed.",
                            color = MaterialTheme.colorScheme.onErrorContainer,
                            style = MaterialTheme.typography.bodySmall,
                            modifier = Modifier.padding(12.dp)
                        )
                    }
                    Spacer(modifier = Modifier.height(12.dp))
                }
                else -> {}
            }

            Text(
                text = "Print Options",
                style = MaterialTheme.typography.titleMedium
            )

            Spacer(modifier = Modifier.height(8.dp))

            // Media Selection
            Row(
                modifier = Modifier.fillMaxWidth(),
                horizontalArrangement = Arrangement.SpaceBetween,
                verticalAlignment = Alignment.CenterVertically
            ) {
                Text("Requested Media")
                AssistChip(
                    onClick = {
                        val nextIdx = (supportedMedia.indexOf(uiState.selectedMedia) + 1) % supportedMedia.size
                        onUpdateMedia(supportedMedia[nextIdx])
                    },
                    label = { Text(uiState.selectedMedia.displayName) }
                )
            }

            // Color Mode Selection
            Row(
                modifier = Modifier.fillMaxWidth(),
                horizontalArrangement = Arrangement.SpaceBetween,
                verticalAlignment = Alignment.CenterVertically
            ) {
                Text("Color Mode")
                AssistChip(
                    onClick = {
                        val nextIdx = (supportedColors.indexOf(uiState.colorMode) + 1) % supportedColors.size
                        onUpdateColorMode(supportedColors[nextIdx])
                    },
                    label = { Text(uiState.colorMode.displayName) }
                )
            }

            // Duplex Selection
            Row(
                modifier = Modifier.fillMaxWidth(),
                horizontalArrangement = Arrangement.SpaceBetween,
                verticalAlignment = Alignment.CenterVertically
            ) {
                Text("Two-Sided (Duplex)")
                AssistChip(
                    onClick = {
                        val nextIdx = (supportedDuplex.indexOf(uiState.duplexMode) + 1) % supportedDuplex.size
                        onUpdateDuplexMode(supportedDuplex[nextIdx])
                    },
                    label = { Text(uiState.duplexMode.displayName) }
                )
            }

            // Orientation Selection
            Row(
                modifier = Modifier.fillMaxWidth(),
                horizontalArrangement = Arrangement.SpaceBetween,
                verticalAlignment = Alignment.CenterVertically
            ) {
                Text("Orientation")
                AssistChip(
                    onClick = {
                        val next = if (uiState.orientation == PrintOrientation.PORTRAIT) {
                            PrintOrientation.LANDSCAPE
                        } else {
                            PrintOrientation.PORTRAIT
                        }
                        onUpdateOrientation(next)
                    },
                    label = { Text(uiState.orientation.displayName) }
                )
            }

            // Copies
            Row(
                modifier = Modifier.fillMaxWidth(),
                horizontalArrangement = Arrangement.SpaceBetween,
                verticalAlignment = Alignment.CenterVertically
            ) {
                Text("Copies")
                Row(verticalAlignment = Alignment.CenterVertically) {
                    IconButton(onClick = { onUpdateCopies(uiState.copies - 1) }) {
                        Text("-", style = MaterialTheme.typography.titleLarge)
                    }
                    Text("${uiState.copies}", modifier = Modifier.padding(horizontal = 8.dp))
                    IconButton(onClick = { onUpdateCopies(uiState.copies + 1) }) {
                        Text("+", style = MaterialTheme.typography.titleLarge)
                    }
                }
            }

            // Fit to page
            Row(
                modifier = Modifier.fillMaxWidth(),
                horizontalArrangement = Arrangement.SpaceBetween,
                verticalAlignment = Alignment.CenterVertically
            ) {
                Text("Fit to Page")
                Switch(
                    checked = uiState.fitToPage,
                    onCheckedChange = onUpdateFitToPage
                )
            }

            Spacer(modifier = Modifier.weight(1f))

            val isWorking = uiState.jobProgress.state == PrintJobState.RASTERIZING ||
                uiState.jobProgress.state == PrintJobState.STREAMING

            Button(
                onClick = onSubmitPrint,
                enabled = !isWorking,
                modifier = Modifier.fillMaxWidth()
            ) {
                Text(if (isWorking) "Sending Print Job..." else "Print to $printerName")
            }
        }
    }
}
