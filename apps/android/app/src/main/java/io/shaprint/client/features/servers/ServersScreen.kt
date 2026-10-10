package io.shaprint.client.features.servers

import androidx.compose.foundation.layout.*
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.unit.dp
import io.shaprint.client.core.domain.model.TrustedServer

@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun ServersScreen(
    onNavigateBack: () -> Unit,
    onNavigateToPrinters: () -> Unit
) {
    val trustedServers = remember {
        mutableStateListOf(
            TrustedServer(
                host = "192.168.1.100",
                port = 48631,
                computerName = "OFFICE-PC",
                sha256Fingerprint = "A1:B2:C3:D4:E5:F6:07:18:29:3A:4B:5C:6D:7E:8F:90:12:34:56:78:9A:BC:DE:F0:12:34:56:78:9A:BC:DE:F0",
                networkChannel = "default-channel"
            )
        )
    }

    Scaffold(
        topBar = {
            TopAppBar(
                title = { Text("ShaPrint — Trusted Servers") },
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
            Card(
                colors = CardDefaults.cardColors(
                    containerColor = MaterialTheme.colorScheme.secondaryContainer
                ),
                modifier = Modifier.fillMaxWidth()
            ) {
                Column(modifier = Modifier.padding(12.dp)) {
                    Text(
                        text = "A trusted server is one whose TLS certificate fingerprint you reviewed and approved.",
                        style = MaterialTheme.typography.bodyMedium
                    )
                    Spacer(modifier = Modifier.height(4.dp))
                    Text(
                        text = "Identity is anchored by certificate pinning, never by unverified discovery.",
                        style = MaterialTheme.typography.labelSmall
                    )
                }
            }

            Spacer(modifier = Modifier.height(16.dp))

            Text(
                text = "Pinned Servers (${trustedServers.size})",
                style = MaterialTheme.typography.titleMedium
            )

            Spacer(modifier = Modifier.height(8.dp))

            if (trustedServers.isEmpty()) {
                Box(
                    modifier = Modifier.fillMaxSize(),
                    contentAlignment = Alignment.Center
                ) {
                    Text("No trusted servers configured yet.")
                }
            } else {
                LazyColumn(
                    verticalArrangement = Arrangement.spacedBy(8.dp),
                    modifier = Modifier.weight(1f)
                ) {
                    items(trustedServers) { server ->
                        Card(modifier = Modifier.fillMaxWidth()) {
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
                                Spacer(modifier = Modifier.height(6.dp))
                                Text(
                                    text = "Fingerprint: ${server.sha256Fingerprint.take(24)}...",
                                    style = MaterialTheme.typography.bodySmall
                                )
                                Text(
                                    text = "Network Channel: ${if (server.networkChannel != null) "Configured" else "Missing"}",
                                    style = MaterialTheme.typography.bodySmall
                                )
                            }
                        }
                    }
                }
            }

            Spacer(modifier = Modifier.height(8.dp))

            Button(
                onClick = onNavigateToPrinters,
                modifier = Modifier.fillMaxWidth()
            ) {
                Text("Select Printer to Print")
            }
        }
    }
}
