package io.shaprint.client.features.discovery

import androidx.compose.foundation.layout.*
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.unit.dp
import io.shaprint.client.core.domain.model.NearbyServer

@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun DiscoveryScreen(
    uiState: DiscoveryUiState = DiscoveryUiState(),
    onStartScan: () -> Unit = {},
    onOpenManualDialog: () -> Unit = {},
    onDismissManualDialog: () -> Unit = {},
    onUpdateManualHost: (String) -> Unit = {},
    onUpdateManualPort: (String) -> Unit = {},
    onSubmitManualServer: () -> Unit = {},
    onNavigateToServers: () -> Unit = {},
    onNavigateToPrinters: () -> Unit = {},
    onServerClick: (NearbyServer) -> Unit = {}
) {
    Scaffold(
        topBar = {
            TopAppBar(
                title = { Text("ShaPrint — Nearby Servers") },
                actions = {
                    TextButton(onClick = onNavigateToServers) {
                        Text("Trusted Servers")
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
            Card(
                colors = CardDefaults.cardColors(
                    containerColor = MaterialTheme.colorScheme.surfaceVariant
                ),
                modifier = Modifier.fillMaxWidth()
            ) {
                Column(modifier = Modifier.padding(12.dp)) {
                    Text(
                        text = "A nearby server is a hint to review, never a trusted server.",
                        style = MaterialTheme.typography.bodyMedium
                    )
                    Spacer(modifier = Modifier.height(4.dp))
                    Text(
                        text = "Discovery uses UDP 48633 (_shaprint-ipps._tcp.local.) with MulticastLock.",
                        style = MaterialTheme.typography.labelSmall
                    )
                }
            }

            Spacer(modifier = Modifier.height(16.dp))

            if (uiState.isScanning) {
                LinearProgressIndicator(modifier = Modifier.fillMaxWidth())
                Spacer(modifier = Modifier.height(12.dp))
            }

            Row(
                modifier = Modifier.fillMaxWidth(),
                horizontalArrangement = Arrangement.SpaceBetween,
                verticalAlignment = Alignment.CenterVertically
            ) {
                Text(
                    text = "Discovered (${uiState.nearbyServers.size})",
                    style = MaterialTheme.typography.titleMedium
                )
                Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                    OutlinedButton(
                        onClick = onOpenManualDialog,
                        enabled = !uiState.isScanning
                    ) {
                        Text("Add Manual IP")
                    }
                    Button(
                        onClick = onStartScan,
                        enabled = !uiState.isScanning
                    ) {
                        Text(if (uiState.isScanning) "Scanning..." else "Scan Network")
                    }
                }
            }

            Spacer(modifier = Modifier.height(12.dp))

            if (uiState.nearbyServers.isEmpty()) {
                Box(
                    modifier = Modifier
                        .weight(1f)
                        .fillMaxWidth(),
                    contentAlignment = Alignment.Center
                ) {
                    Column(horizontalAlignment = Alignment.CenterHorizontally) {
                        Text("No nearby servers found.")
                        Spacer(modifier = Modifier.height(6.dp))
                        Text(
                            text = "Tap 'Scan Network' to search local Wi-Fi or 'Add Manual IP' for cross-VLAN.",
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
                    items(uiState.nearbyServers) { server ->
                        Card(
                            onClick = { onServerClick(server) },
                            modifier = Modifier.fillMaxWidth()
                        ) {
                            Column(modifier = Modifier.padding(16.dp)) {
                                Row(
                                    modifier = Modifier.fillMaxWidth(),
                                    horizontalArrangement = Arrangement.SpaceBetween
                                ) {
                                    Text(
                                        text = server.computerName,
                                        style = MaterialTheme.typography.titleMedium
                                    )
                                    Text(
                                        text = if (server.isOnline) "ONLINE" else "OFFLINE",
                                        style = MaterialTheme.typography.labelSmall,
                                        color = if (server.isOnline) MaterialTheme.colorScheme.primary else MaterialTheme.colorScheme.error
                                    )
                                }
                                Text(
                                    text = server.canonicalAddress,
                                    style = MaterialTheme.typography.bodySmall,
                                    color = MaterialTheme.colorScheme.outline
                                )
                                if (server.advertisedQueues.isNotEmpty()) {
                                    Spacer(modifier = Modifier.height(4.dp))
                                    Text(
                                        text = "Shared Queues: ${server.advertisedQueues.joinToString(", ")}",
                                        style = MaterialTheme.typography.bodyMedium
                                    )
                                }
                            }
                        }
                    }
                }
            }

            Spacer(modifier = Modifier.height(8.dp))

            OutlinedButton(
                onClick = onNavigateToPrinters,
                modifier = Modifier.fillMaxWidth()
            ) {
                Text("View Available Printers")
            }
        }
    }

    if (uiState.showManualServerDialog) {
        AlertDialog(
            onDismissRequest = onDismissManualDialog,
            title = { Text("Add Server by Address") },
            text = {
                Column {
                    Text(
                        text = "Enter server IP address or hostname across VLANs:",
                        style = MaterialTheme.typography.bodySmall
                    )
                    Spacer(modifier = Modifier.height(8.dp))
                    OutlinedTextField(
                        value = uiState.manualHostInput,
                        onValueChange = onUpdateManualHost,
                        label = { Text("Server Host / IP") },
                        placeholder = { Text("192.168.1.100") },
                        singleLine = true,
                        modifier = Modifier.fillMaxWidth()
                    )
                    Spacer(modifier = Modifier.height(8.dp))
                    OutlinedTextField(
                        value = uiState.manualPortInput,
                        onValueChange = onUpdateManualPort,
                        label = { Text("IPPS Port (Default 48631)") },
                        singleLine = true,
                        modifier = Modifier.fillMaxWidth()
                    )
                    if (uiState.manualInputError != null) {
                        Spacer(modifier = Modifier.height(4.dp))
                        Text(
                            text = uiState.manualInputError,
                            color = MaterialTheme.colorScheme.error,
                            style = MaterialTheme.typography.labelSmall
                        )
                    }
                }
            },
            confirmButton = {
                Button(onClick = onSubmitManualServer) {
                    Text("Add & Probe")
                }
            },
            dismissButton = {
                TextButton(onClick = onDismissManualDialog) {
                    Text("Cancel")
                }
            }
        )
    }
}
