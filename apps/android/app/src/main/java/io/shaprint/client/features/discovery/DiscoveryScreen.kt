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
    onNavigateToServers: () -> Unit,
    onNavigateToPrinters: () -> Unit,
    onServerClick: (NearbyServer) -> Unit = {}
) {
    var isScanning by remember { mutableStateOf(false) }
    val nearbyServers = remember {
        mutableStateListOf(
            NearbyServer(
                host = "192.168.1.100",
                port = 48631,
                computerName = "OFFICE-PC",
                advertisedQueues = listOf("Epson-L3210", "HP-LaserJet")
            )
        )
    }

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
                    Spacer(modifier = Modifier.height(8.dp))
                    Text(
                        text = "Multicast DNS discovery runs on UDP port 48633.",
                        style = MaterialTheme.typography.labelSmall
                    )
                }
            }

            Spacer(modifier = Modifier.height(16.dp))

            Row(
                modifier = Modifier.fillMaxWidth(),
                horizontalArrangement = Arrangement.SpaceBetween,
                verticalAlignment = Alignment.CenterVertically
            ) {
                Text(
                    text = "Discovered on Wi-Fi (${nearbyServers.size})",
                    style = MaterialTheme.typography.titleMedium
                )
                Button(
                    onClick = { isScanning = !isScanning },
                    enabled = !isScanning
                ) {
                    Text(if (isScanning) "Scanning..." else "Scan Network")
                }
            }

            Spacer(modifier = Modifier.height(12.dp))

            if (nearbyServers.isEmpty()) {
                Box(
                    modifier = Modifier.fillMaxSize(),
                    contentAlignment = Alignment.Center
                ) {
                    Text("No nearby servers found. Tap Scan to search local Wi-Fi.")
                }
            } else {
                LazyColumn(
                    verticalArrangement = Arrangement.spacedBy(8.dp),
                    modifier = Modifier.weight(1f)
                ) {
                    items(nearbyServers) { server ->
                        Card(
                            onClick = { onServerClick(server) },
                            modifier = Modifier.fillMaxWidth()
                        ) {
                            Column(modifier = Modifier.padding(16.dp)) {
                                Text(
                                    text = server.computerName,
                                    style = MaterialTheme.typography.titleMedium
                                )
                                Text(
                                    text = server.canonicalAddress,
                                    style = MaterialTheme.typography.bodySmall,
                                    color = MaterialTheme.colorScheme.outline
                                )
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

            Spacer(modifier = Modifier.height(8.dp))

            OutlinedButton(
                onClick = onNavigateToPrinters,
                modifier = Modifier.fillMaxWidth()
            ) {
                Text("View Available Printers")
            }
        }
    }
}
