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
    uiState: ServersUiState = ServersUiState(),
    onApprovePendingServer: () -> Unit = {},
    onDismissReviewDialog: () -> Unit = {},
    onUpdateChannelInput: (String) -> Unit = {},
    onForgetServer: (String) -> Unit = {},
    onNavigateBack: () -> Unit = {},
    onNavigateToPrinters: () -> Unit = {}
) {
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
                text = "Pinned Servers (${uiState.trustedServers.size})",
                style = MaterialTheme.typography.titleMedium
            )

            Spacer(modifier = Modifier.height(8.dp))

            if (uiState.trustedServers.isEmpty()) {
                Box(
                    modifier = Modifier
                        .weight(1f)
                        .fillMaxWidth(),
                    contentAlignment = Alignment.Center
                ) {
                    Text("No trusted servers configured yet. Review a nearby server to trust it.")
                }
            } else {
                LazyColumn(
                    verticalArrangement = Arrangement.spacedBy(8.dp),
                    modifier = Modifier.weight(1f)
                ) {
                    items(uiState.trustedServers) { server ->
                        Card(modifier = Modifier.fillMaxWidth()) {
                            Column(modifier = Modifier.padding(16.dp)) {
                                Row(
                                    modifier = Modifier.fillMaxWidth(),
                                    horizontalArrangement = Arrangement.SpaceBetween,
                                    verticalAlignment = Alignment.CenterVertically
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
                                    text = "Fingerprint: ${server.sha256Fingerprint}",
                                    style = MaterialTheme.typography.bodySmall
                                )
                                Text(
                                    text = "Network Channel: ${if (server.networkChannel != null) "Configured (Encrypted)" else "None"}",
                                    style = MaterialTheme.typography.bodySmall
                                )
                                Spacer(modifier = Modifier.height(8.dp))
                                Row(
                                    modifier = Modifier.fillMaxWidth(),
                                    horizontalArrangement = Arrangement.End
                                ) {
                                    TextButton(
                                        onClick = { onForgetServer(server.canonicalAddress) },
                                        colors = ButtonDefaults.textButtonColors(
                                            contentColor = MaterialTheme.colorScheme.error
                                        )
                                    ) {
                                        Text("Forget Server")
                                    }
                                }
                            }
                        }
                    }
                }
            }

            Spacer(modifier = Modifier.height(8.dp))

            Button(
                onClick = onNavigateToPrinters,
                modifier = Modifier.fillMaxWidth(),
                enabled = uiState.trustedServers.isNotEmpty()
            ) {
                Text("Select Printer to Print")
            }
        }
    }

    // TOFU Review Dialog
    if (uiState.pendingReviewServer != null) {
        val server = uiState.pendingReviewServer
        AlertDialog(
            onDismissRequest = onDismissReviewDialog,
            title = { Text("Trust Server: ${server.computerName}") },
            text = {
                Column {
                    Text(
                        text = "Address: ${server.canonicalAddress}",
                        style = MaterialTheme.typography.bodyMedium
                    )
                    Spacer(modifier = Modifier.height(8.dp))

                    if (uiState.inspectingFingerprint) {
                        Row(verticalAlignment = Alignment.CenterVertically) {
                            CircularProgressIndicator(modifier = Modifier.size(20.dp))
                            Spacer(modifier = Modifier.width(8.dp))
                            Text("Inspecting TLS certificate on port ${server.port}...")
                        }
                    } else if (uiState.inspectionError != null) {
                        Text(
                            text = "Failed to inspect certificate: ${uiState.inspectionError}",
                            color = MaterialTheme.colorScheme.error,
                            style = MaterialTheme.typography.bodySmall
                        )
                    } else if (uiState.inspectedFingerprint != null) {
                        Text(
                            text = "SHA-256 Certificate Fingerprint:",
                            style = MaterialTheme.typography.labelMedium
                        )
                        Card(
                            colors = CardDefaults.cardColors(
                                containerColor = MaterialTheme.colorScheme.surfaceVariant
                            ),
                            modifier = Modifier.fillMaxWidth().padding(vertical = 4.dp)
                        ) {
                            Text(
                                text = uiState.inspectedFingerprint,
                                style = MaterialTheme.typography.bodySmall,
                                modifier = Modifier.padding(8.dp)
                            )
                        }

                        if (uiState.securityAlert != null) {
                            Spacer(modifier = Modifier.height(6.dp))
                            Card(
                                colors = CardDefaults.cardColors(
                                    containerColor = MaterialTheme.colorScheme.errorContainer
                                ),
                                modifier = Modifier.fillMaxWidth()
                            ) {
                                Text(
                                    text = uiState.securityAlert,
                                    color = MaterialTheme.colorScheme.onErrorContainer,
                                    style = MaterialTheme.typography.bodySmall,
                                    modifier = Modifier.padding(8.dp)
                                )
                            }
                        }

                        Spacer(modifier = Modifier.height(12.dp))
                        OutlinedTextField(
                            value = uiState.channelInput,
                            onValueChange = onUpdateChannelInput,
                            label = { Text("Network Channel (Optional)") },
                            placeholder = { Text("Enter shared secret") },
                            singleLine = true,
                            modifier = Modifier.fillMaxWidth()
                        )
                    }
                }
            },
            confirmButton = {
                Button(
                    onClick = onApprovePendingServer,
                    enabled = uiState.inspectedFingerprint != null
                ) {
                    Text("Approve & Trust")
                }
            },
            dismissButton = {
                TextButton(onClick = onDismissReviewDialog) {
                    Text("Cancel")
                }
            }
        )
    }
}
