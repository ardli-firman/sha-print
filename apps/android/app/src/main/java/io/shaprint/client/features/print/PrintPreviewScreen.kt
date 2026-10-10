package io.shaprint.client.features.print

import androidx.compose.foundation.background
import androidx.compose.foundation.layout.*
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.unit.dp
import io.shaprint.client.core.domain.model.PrintColorMode
import io.shaprint.client.core.domain.model.RequestedMedia

@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun PrintPreviewScreen(
    printerName: String = "Epson L3210 Series",
    onNavigateBack: () -> Unit,
    onSubmitPrint: () -> Unit = {}
) {
    var selectedMedia by remember { mutableStateOf(RequestedMedia.ISO_A4) }
    var selectedColorMode by remember { mutableStateOf(PrintColorMode.COLOR) }
    var copies by remember { mutableIntStateOf(1) }
    var fitToPage by remember { mutableStateOf(true) }

    Scaffold(
        topBar = {
            TopAppBar(
                title = { Text("Print — $printerName") },
                navigationIcon = {
                    TextButton(onClick = onNavigateBack) {
                        Text("Back")
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
                    .height(200.dp)
                    .background(
                        color = MaterialTheme.colorScheme.surfaceVariant,
                        shape = MaterialTheme.shapes.medium
                    ),
                contentAlignment = Alignment.Center
            ) {
                Column(horizontalAlignment = Alignment.CenterHorizontally) {
                    Text(
                        text = "Document Preview",
                        style = MaterialTheme.typography.titleMedium
                    )
                    Text(
                        text = "Format: 300 DPI PWG Raster (ADR 0010)",
                        style = MaterialTheme.typography.bodySmall,
                        color = MaterialTheme.colorScheme.outline
                    )
                    Text(
                        text = "${selectedMedia.displayName} • ${selectedColorMode.displayName}",
                        style = MaterialTheme.typography.bodySmall
                    )
                }
            }

            Spacer(modifier = Modifier.height(20.dp))

            Text(
                text = "Print Options",
                style = MaterialTheme.typography.titleMedium
            )

            Spacer(modifier = Modifier.height(12.dp))

            // Media Selection
            Row(
                modifier = Modifier.fillMaxWidth(),
                horizontalArrangement = Arrangement.SpaceBetween,
                verticalAlignment = Alignment.CenterVertically
            ) {
                Text("Requested Media")
                AssistChip(
                    onClick = {
                        selectedMedia = when (selectedMedia) {
                            RequestedMedia.ISO_A4 -> RequestedMedia.OM_FOLIO
                            RequestedMedia.OM_FOLIO -> RequestedMedia.NA_LETTER
                            else -> RequestedMedia.ISO_A4
                        }
                    },
                    label = { Text(selectedMedia.displayName) }
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
                        selectedColorMode = if (selectedColorMode == PrintColorMode.COLOR) {
                            PrintColorMode.MONOCHROME
                        } else {
                            PrintColorMode.COLOR
                        }
                    },
                    label = { Text(selectedColorMode.displayName) }
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
                    IconButton(onClick = { if (copies > 1) copies-- }) {
                        Text("-", style = MaterialTheme.typography.titleLarge)
                    }
                    Text("$copies", modifier = Modifier.padding(horizontal = 8.dp))
                    IconButton(onClick = { if (copies < 99) copies++ }) {
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
                Switch(checked = fitToPage, onCheckedChange = { fitToPage = it })
            }

            Spacer(modifier = Modifier.weight(1f))

            Button(
                onClick = onSubmitPrint,
                modifier = Modifier.fillMaxWidth()
            ) {
                Text("Print to $printerName")
            }
        }
    }
}
