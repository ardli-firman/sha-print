package io.shaprint.client.features.printers

import androidx.lifecycle.ViewModel
import androidx.lifecycle.viewModelScope
import io.shaprint.client.core.domain.model.SharedPrinter
import io.shaprint.client.core.network.IppsClient
import io.shaprint.client.core.security.TrustStore
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.flow.update
import kotlinx.coroutines.launch

data class PrintersUiState(
    val printers: List<SharedPrinter> = emptyList(),
    val isLoading: Boolean = false,
    val selectedPrinter: SharedPrinter? = null,
    val errorMessage: String? = null
)

class PrintersViewModel(
    private val trustStore: TrustStore,
    private val ippsClient: IppsClient
) : ViewModel() {

    private val _uiState = MutableStateFlow(PrintersUiState())
    val uiState: StateFlow<PrintersUiState> = _uiState.asStateFlow()

    init {
        loadPrinters()
    }

    fun loadPrinters() {
        viewModelScope.launch {
            _uiState.update { it.copy(isLoading = true, errorMessage = null) }
            try {
                val servers = trustStore.getTrustedServers()
                val allPrinters = mutableListOf<SharedPrinter>()

                for (server in servers) {
                    try {
                        val queried = ippsClient.queryPrinters(server)
                        allPrinters.addAll(queried)
                    } catch (e: Exception) {
                        // Mark server printers as offline or surface note
                    }
                }

                _uiState.update {
                    it.copy(
                        printers = allPrinters,
                        isLoading = false
                    )
                }
            } catch (e: Exception) {
                _uiState.update {
                    it.copy(
                        isLoading = false,
                        errorMessage = e.message ?: "Failed to query shared printers"
                    )
                }
            }
        }
    }

    fun selectPrinter(printer: SharedPrinter) {
        _uiState.update { it.copy(selectedPrinter = printer) }
    }
}
