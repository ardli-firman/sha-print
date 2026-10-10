package io.shaprint.client.features.printers

import androidx.compose.foundation.layout.*
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.unit.dp
import io.shaprint.client.core.domain.model.SharedPrinter

@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun PrintersScreen(
    uiState: PrintersUiState = PrintersUiState(),
    onRefresh: () -> Unit = {},
    onNavigateBack: () -> Unit = {},
    onSelectPrinter: (SharedPrinter) -> Unit = {}
) {
    Scaffold(
        topBar = {
            TopAppBar(
                title = { Text("ShaPrint — Shared Printers") },
                navigationIcon = {
                    TextButton(onClick = onNavigateBack) {
                        Text("Back")
                    }
                },
                actions = {
                    TextButton(onClick = onRefresh, enabled = !uiState.isLoading) {
                        Text("Refresh")
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
                text = "Available Shared Printers (${uiState.printers.size})",
                style = MaterialTheme.typography.titleMedium
            )
            Text(
                text = "Queried dynamically from your trusted servers over IPPS port 48631.",
                style = MaterialTheme.typography.bodySmall,
                color = MaterialTheme.colorScheme.outline
            )

            Spacer(modifier = Modifier.height(12.dp))

            if (uiState.isLoading) {
                LinearProgressIndicator(modifier = Modifier.fillMaxWidth())
                Spacer(modifier = Modifier.height(12.dp))
            }

            if (uiState.errorMessage != null) {
                Card(
                    colors = CardDefaults.cardColors(
                        containerColor = MaterialTheme.colorScheme.errorContainer
                    ),
                    modifier = Modifier.fillMaxWidth()
                ) {
                    Text(
                        text = uiState.errorMessage,
                        color = MaterialTheme.colorScheme.onErrorContainer,
                        style = MaterialTheme.typography.bodySmall,
                        modifier = Modifier.padding(12.dp)
                    )
                }
                Spacer(modifier = Modifier.height(12.dp))
            }

            if (uiState.printers.isEmpty() && !uiState.isLoading) {
                Box(
                    modifier = Modifier
                        .fillMaxSize()
                        .weight(1f),
                    contentAlignment = Alignment.Center
                ) {
                    Column(horizontalAlignment = Alignment.CenterHorizontally) {
                        Text("No shared printers found on trusted servers.")
                        Spacer(modifier = Modifier.height(6.dp))
                        Text(
                            text = "Ensure your trusted servers are running and sharing print queues.",
                            style = MaterialTheme.typography.bodySmall,
                            color = MaterialTheme.colorScheme.outline
                        )
                    }
                }
            } else {
                LazyColumn(
                    verticalArrangement = Arrangement.spacedBy(8.dp),
                    modifier = Modifier.weight(1f)
                ) {
                    items(uiState.printers) { printer ->
                        Card(
                            onClick = { onSelectPrinter(printer) },
                            modifier = Modifier.fillMaxWidth()
                        ) {
                            Column(modifier = Modifier.padding(16.dp)) {
                                Row(
                                    modifier = Modifier.fillMaxWidth(),
                                    horizontalArrangement = Arrangement.SpaceBetween,
                                    verticalAlignment = Alignment.CenterVertically
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
                                Spacer(modifier = Modifier.height(8.dp))

                                val mediaNames = printer.capabilities.supportedMedia.map { it.displayName }
                                Text(
                                    text = "Supported Media: ${mediaNames.joinToString(", ")}",
                                    style = MaterialTheme.typography.bodySmall
                                )
                                val colorNames = printer.capabilities.supportedColorModes.map { it.displayName }
                                Text(
                                    text = "Color Modes: ${colorNames.joinToString(", ")}",
                                    style = MaterialTheme.typography.bodySmall
                                )
                                val duplexNames = printer.capabilities.supportedDuplexModes.map { it.displayName }
                                Text(
                                    text = "Duplex: ${duplexNames.joinToString(", ")}",
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
