package io.shaprint.client.features.printers

import androidx.compose.foundation.layout.*
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.unit.dp
import io.shaprint.client.core.domain.model.RequestedMedia
import io.shaprint.client.core.domain.model.SharedPrinter

@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun PrintersScreen(
    onNavigateBack: () -> Unit,
    onSelectPrinter: (SharedPrinter) -> Unit
) {
    val printers = remember {
        mutableStateListOf(
            SharedPrinter(
                name = "Epson L3210 Series",
                serverCanonicalAddress = "192.168.1.100:48631",
                isOnline = true
            ),
            SharedPrinter(
                name = "HP LaserJet Pro M404",
                serverCanonicalAddress = "192.168.1.100:48631",
                isOnline = true
            )
        )
    }

    Scaffold(
        topBar = {
            TopAppBar(
                title = { Text("ShaPrint — Shared Printers") },
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
            Text(
                text = "Available Shared Printers",
                style = MaterialTheme.typography.titleMedium
            )
            Text(
                text = "Printers shared by your trusted servers on port 48631.",
                style = MaterialTheme.typography.bodySmall,
                color = MaterialTheme.colorScheme.outline
            )

            Spacer(modifier = Modifier.height(16.dp))

            if (printers.isEmpty()) {
                Box(
                    modifier = Modifier.fillMaxSize(),
                    contentAlignment = Alignment.Center
                ) {
                    Text("No shared printers found on trusted servers.")
                }
            } else {
                LazyColumn(
                    verticalArrangement = Arrangement.spacedBy(8.dp),
                    modifier = Modifier.fillMaxSize()
                ) {
                    items(printers) { printer ->
                        Card(
                            onClick = { onSelectPrinter(printer) },
                            modifier = Modifier.fillMaxWidth()
                        ) {
                            Column(modifier = Modifier.padding(16.dp)) {
                                Row(
                                    modifier = Modifier.fillMaxWidth(),
                                    horizontalArrangement = Arrangement.SpaceBetween
                                ) {
                                    Text(
                                        text = printer.name,
                                        style = MaterialTheme.typography.titleMedium
                                    )
                                    Text(
                                        text = if (printer.isOnline) "Ready" else "Offline",
                                        style = MaterialTheme.typography.labelSmall,
                                        color = if (printer.isOnline) MaterialTheme.colorScheme.primary else MaterialTheme.colorScheme.error
                                    )
                                }
                                Text(
                                    text = "Server: ${printer.serverCanonicalAddress}",
                                    style = MaterialTheme.typography.bodySmall,
                                    color = MaterialTheme.colorScheme.outline
                                )
                                Spacer(modifier = Modifier.height(6.dp))
                                Text(
                                    text = "Supported: A4, Letter, F4/Folio (300 DPI PWG Raster)",
                                    style = MaterialTheme.typography.bodySmall
                                )
                            }
                        }
                    }
                }
            }
        }
    }
}
