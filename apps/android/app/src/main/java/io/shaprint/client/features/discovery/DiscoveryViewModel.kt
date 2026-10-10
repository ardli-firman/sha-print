package io.shaprint.client.features.discovery

import androidx.lifecycle.ViewModel
import androidx.lifecycle.viewModelScope
import io.shaprint.client.core.domain.model.NearbyServer
import io.shaprint.client.core.network.Ports
import io.shaprint.client.core.network.UnicastProbe
import io.shaprint.client.core.network.mdns.MdnsScanner
import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.Job
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.flow.update
import kotlinx.coroutines.launch

data class DiscoveryUiState(
    val isScanning: Boolean = false,
    val nearbyServers: List<NearbyServer> = emptyList(),
    val errorMessage: String? = null,
    val showManualServerDialog: Boolean = false,
    val manualHostInput: String = "",
    val manualPortInput: String = Ports.IPPS_SERVER_DEFAULT_PORT.toString(),
    val manualInputError: String? = null
)

class DiscoveryViewModel(
    private val mdnsScanner: MdnsScanner,
    private val unicastProbe: UnicastProbe
) : ViewModel() {

    private val _uiState = MutableStateFlow(DiscoveryUiState())
    val uiState: StateFlow<DiscoveryUiState> = _uiState.asStateFlow()

    private var scanJob: Job? = null

    fun startScan(timeoutMs: Long = 10000L) {
        scanJob?.cancel()
        _uiState.update { it.copy(isScanning = true, errorMessage = null) }

        scanJob = viewModelScope.launch {
            try {
                mdnsScanner.scan(timeoutMs).collect { discovered ->
                    _uiState.update { current ->
                        val existingIndex = current.nearbyServers.indexOfFirst {
                            it.canonicalAddress == discovered.canonicalAddress
                        }
                        val updatedList = if (existingIndex >= 0) {
                            current.nearbyServers.toMutableList().apply {
                                set(existingIndex, discovered)
                            }
                        } else {
                            current.nearbyServers + discovered
                        }
                        current.copy(nearbyServers = updatedList)
                    }
                }
            } catch (e: CancellationException) {
                throw e
            } catch (e: Exception) {
                _uiState.update { it.copy(errorMessage = e.message ?: "Discovery scan failed") }
            } finally {
                _uiState.update { it.copy(isScanning = false) }
            }
        }
    }

    fun stopScan() {
        scanJob?.cancel()
        scanJob = null
        _uiState.update { it.copy(isScanning = false) }
    }

    fun openManualServerDialog() {
        _uiState.update {
            it.copy(
                showManualServerDialog = true,
                manualHostInput = "",
                manualPortInput = Ports.IPPS_SERVER_DEFAULT_PORT.toString(),
                manualInputError = null
            )
        }
    }

    fun dismissManualServerDialog() {
        _uiState.update { it.copy(showManualServerDialog = false, manualInputError = null) }
    }

    fun updateManualHost(host: String) {
        _uiState.update { it.copy(manualHostInput = host, manualInputError = null) }
    }

    fun updateManualPort(port: String) {
        _uiState.update { it.copy(manualPortInput = port, manualInputError = null) }
    }

    fun submitManualServer() {
        val state = _uiState.value
        val host = state.manualHostInput.trim()
        if (host.isBlank()) {
            _uiState.update { it.copy(manualInputError = "Host / IP address cannot be empty") }
            return
        }

        val port = state.manualPortInput.trim().toIntOrNull()
        if (port == null || port !in 1..65535) {
            _uiState.update { it.copy(manualInputError = "Port must be a number between 1 and 65535") }
            return
        }

        viewModelScope.launch {
            val isOnline = unicastProbe.probe(host, port)
            val manualServer = NearbyServer(
                host = host,
                port = port,
                computerName = host,
                advertisedQueues = emptyList(),
                isOnline = isOnline
            )

            _uiState.update { current ->
                val existingIndex = current.nearbyServers.indexOfFirst {
                    it.canonicalAddress == manualServer.canonicalAddress
                }
                val updatedList = if (existingIndex >= 0) {
                    current.nearbyServers.toMutableList().apply {
                        set(existingIndex, manualServer)
                    }
                } else {
                    current.nearbyServers + manualServer
                }
                current.copy(
                    nearbyServers = updatedList,
                    showManualServerDialog = false,
                    manualInputError = null
                )
            }
        }
    }

    fun checkReachability(server: NearbyServer) {
        viewModelScope.launch {
            val isOnline = unicastProbe.probe(server.host, server.port)
            _uiState.update { current ->
                val updated = current.nearbyServers.map {
                    if (it.canonicalAddress == server.canonicalAddress) it.copy(isOnline = isOnline) else it
                }
                current.copy(nearbyServers = updated)
            }
        }
    }
}
