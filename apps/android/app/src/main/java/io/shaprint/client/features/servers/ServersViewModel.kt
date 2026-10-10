package io.shaprint.client.features.servers

import androidx.lifecycle.ViewModel
import androidx.lifecycle.viewModelScope
import io.shaprint.client.core.domain.model.NearbyServer
import io.shaprint.client.core.domain.model.TrustedServer
import io.shaprint.client.core.network.CertificateInspectionResult
import io.shaprint.client.core.network.TlsCertificateInspector
import io.shaprint.client.core.network.TlsFingerprint
import io.shaprint.client.core.security.TrustStore
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.flow.update
import kotlinx.coroutines.launch

data class ServersUiState(
    val trustedServers: List<TrustedServer> = emptyList(),
    val isLoading: Boolean = false,
    val pendingReviewServer: NearbyServer? = null,
    val inspectingFingerprint: Boolean = false,
    val inspectedFingerprint: String? = null,
    val inspectionError: String? = null,
    val channelInput: String = "",
    val securityAlert: String? = null
)

class ServersViewModel(
    private val trustStore: TrustStore,
    private val tlsInspector: TlsCertificateInspector
) : ViewModel() {

    private val _uiState = MutableStateFlow(ServersUiState())
    val uiState: StateFlow<ServersUiState> = _uiState.asStateFlow()

    init {
        loadTrustedServers()
    }

    fun loadTrustedServers() {
        viewModelScope.launch {
            _uiState.update { it.copy(isLoading = true) }
            val servers = trustStore.getTrustedServers()
            _uiState.update { it.copy(trustedServers = servers, isLoading = false) }
        }
    }

    fun reviewServer(server: NearbyServer) {
        viewModelScope.launch {
            val existing = trustStore.findByCanonicalAddress(server.canonicalAddress)
            _uiState.update {
                it.copy(
                    pendingReviewServer = server,
                    inspectingFingerprint = true,
                    inspectedFingerprint = null,
                    inspectionError = null,
                    securityAlert = null,
                    channelInput = existing?.networkChannel ?: ""
                )
            }

            when (val result = tlsInspector.inspectCertificate(server.host, server.port)) {
                is CertificateInspectionResult.Success -> {
                    if (existing != null && !TlsFingerprint.matches(existing.sha256Fingerprint, result.sha256Fingerprint)) {
                        _uiState.update {
                            it.copy(
                                inspectingFingerprint = false,
                                inspectedFingerprint = result.sha256Fingerprint,
                                securityAlert = "SECURITY WARNING: The certificate fingerprint for ${server.canonicalAddress} has CHANGED from previously approved identity! Do not approve unless the server was deliberately reinstalled."
                            )
                        }
                    } else {
                        _uiState.update {
                            it.copy(
                                inspectingFingerprint = false,
                                inspectedFingerprint = result.sha256Fingerprint
                            )
                        }
                    }
                }
                is CertificateInspectionResult.Failure -> {
                    _uiState.update {
                        it.copy(
                            inspectingFingerprint = false,
                            inspectionError = result.reason
                        )
                    }
                }
            }
        }
    }

    fun updateChannelInput(channel: String) {
        _uiState.update { it.copy(channelInput = channel) }
    }

    fun approvePendingServer() {
        val state = _uiState.value
        val server = state.pendingReviewServer ?: return
        val fingerprint = state.inspectedFingerprint ?: return

        viewModelScope.launch {
            val trusted = TrustedServer(
                host = server.host,
                port = server.port,
                computerName = server.computerName,
                sha256Fingerprint = fingerprint,
                networkChannel = state.channelInput.trim().ifEmpty { null },
                isOnline = true
            )

            trustStore.saveTrustedServer(trusted)
            _uiState.update {
                it.copy(
                    pendingReviewServer = null,
                    inspectedFingerprint = null,
                    securityAlert = null,
                    channelInput = ""
                )
            }
            loadTrustedServers()
        }
    }

    fun dismissReviewDialog() {
        _uiState.update {
            it.copy(
                pendingReviewServer = null,
                inspectedFingerprint = null,
                inspectionError = null,
                securityAlert = null,
                channelInput = ""
            )
        }
    }

    fun forgetServer(canonicalAddress: String) {
        viewModelScope.launch {
            trustStore.removeTrustedServer(canonicalAddress)
            loadTrustedServers()
        }
    }
}
