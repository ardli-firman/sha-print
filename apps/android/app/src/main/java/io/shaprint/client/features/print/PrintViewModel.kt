package io.shaprint.client.features.print

import androidx.lifecycle.ViewModel
import androidx.lifecycle.viewModelScope
import io.shaprint.client.core.domain.model.DuplexMode
import io.shaprint.client.core.domain.model.PrintColorMode
import io.shaprint.client.core.domain.model.PrintJobProgress
import io.shaprint.client.core.domain.model.PrintJobRequest
import io.shaprint.client.core.domain.model.PrintJobState
import io.shaprint.client.core.domain.model.PrintOrientation
import io.shaprint.client.core.domain.model.RequestedMedia
import io.shaprint.client.core.domain.model.SharedPrinter
import io.shaprint.client.core.network.IppsClient
import io.shaprint.client.core.security.TrustStore
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.flow.update
import kotlinx.coroutines.launch
import java.io.OutputStream

data class PrintUiState(
    val selectedPrinter: SharedPrinter? = null,
    val documentName: String = "Sample-Document.pdf",
    val documentUriString: String? = null,
    val totalPages: Int = 1,
    val copies: Int = 1,
    val selectedMedia: RequestedMedia = RequestedMedia.ISO_A4,
    val colorMode: PrintColorMode = PrintColorMode.COLOR,
    val duplexMode: DuplexMode = DuplexMode.ONE_SIDED,
    val orientation: PrintOrientation = PrintOrientation.PORTRAIT,
    val fitToPage: Boolean = true,
    val jobProgress: PrintJobProgress = PrintJobProgress()
)

class PrintViewModel(
    private val trustStore: TrustStore,
    private val ippsClient: IppsClient
) : ViewModel() {

    private val _uiState = MutableStateFlow(PrintUiState())
    val uiState: StateFlow<PrintUiState> = _uiState.asStateFlow()

    fun setPrinter(printer: SharedPrinter) {
        val defaultMedia = printer.capabilities.supportedMedia.firstOrNull() ?: RequestedMedia.ISO_A4
        val defaultColor = printer.capabilities.supportedColorModes.firstOrNull() ?: PrintColorMode.COLOR
        val defaultDuplex = printer.capabilities.supportedDuplexModes.firstOrNull() ?: DuplexMode.ONE_SIDED

        _uiState.update {
            it.copy(
                selectedPrinter = printer,
                selectedMedia = defaultMedia,
                colorMode = defaultColor,
                duplexMode = defaultDuplex
            )
        }
    }

    fun selectDocument(name: String, uriString: String, pageCount: Int = 1) {
        _uiState.update {
            it.copy(
                documentName = name,
                documentUriString = uriString,
                totalPages = pageCount.coerceAtLeast(1),
                jobProgress = PrintJobProgress(state = PrintJobState.IDLE)
            )
        }
    }

    fun updateCopies(copies: Int) {
        val maxCopies = _uiState.value.selectedPrinter?.capabilities?.maxCopies ?: 99
        _uiState.update { it.copy(copies = copies.coerceIn(1, maxCopies)) }
    }

    fun updateMedia(media: RequestedMedia) {
        _uiState.update { it.copy(selectedMedia = media) }
    }

    fun updateColorMode(colorMode: PrintColorMode) {
        _uiState.update { it.copy(colorMode = colorMode) }
    }

    fun updateDuplexMode(duplexMode: DuplexMode) {
        _uiState.update { it.copy(duplexMode = duplexMode) }
    }

    fun updateOrientation(orientation: PrintOrientation) {
        _uiState.update { it.copy(orientation = orientation) }
    }

    fun updateFitToPage(fitToPage: Boolean) {
        _uiState.update { it.copy(fitToPage = fitToPage) }
    }

    fun submitPrintJob(streamRasterGenerator: (OutputStream) -> Unit) {
        val state = _uiState.value
        val printer = state.selectedPrinter ?: run {
            _uiState.update {
                it.copy(
                    jobProgress = PrintJobProgress(
                        state = PrintJobState.FAILED,
                        errorMessage = "Please select a shared printer first."
                    )
                )
            }
            return
        }

        viewModelScope.launch {
            _uiState.update {
                it.copy(
                    jobProgress = PrintJobProgress(
                        state = PrintJobState.RASTERIZING,
                        currentPage = 1,
                        totalPages = state.totalPages
                    )
                )
            }

            try {
                val trustedServer = trustStore.findByCanonicalAddress(printer.serverCanonicalAddress)
                if (trustedServer == null) {
                    _uiState.update {
                        it.copy(
                            jobProgress = PrintJobProgress(
                                state = PrintJobState.FAILED,
                                errorMessage = "Server ${printer.serverCanonicalAddress} is not in your Trusted Servers store."
                            )
                        )
                    }
                    return@launch
                }

                val request = PrintJobRequest(
                    printerName = printer.name,
                    serverCanonicalAddress = printer.serverCanonicalAddress,
                    jobName = state.documentName,
                    copies = state.copies,
                    media = state.selectedMedia,
                    colorMode = state.colorMode,
                    duplexMode = state.duplexMode,
                    orientation = state.orientation,
                    fitToPage = state.fitToPage
                )

                _uiState.update {
                    it.copy(
                        jobProgress = PrintJobProgress(
                            state = PrintJobState.STREAMING,
                            currentPage = state.totalPages,
                            totalPages = state.totalPages
                        )
                    )
                }

                val result = ippsClient.submitPrintJob(
                    server = trustedServer,
                    jobRequest = request,
                    streamRasterPayload = streamRasterGenerator
                )

                if (result.isSuccess) {
                    _uiState.update {
                        it.copy(
                            jobProgress = PrintJobProgress(
                                state = PrintJobState.COMPLETED,
                                currentPage = state.totalPages,
                                totalPages = state.totalPages
                            )
                        )
                    }
                } else {
                    _uiState.update {
                        it.copy(
                            jobProgress = PrintJobProgress(
                                state = PrintJobState.FAILED,
                                errorMessage = result.errorMessage ?: "Print job rejected by server."
                            )
                        )
                    }
                }
            } catch (e: Exception) {
                _uiState.update {
                    it.copy(
                        jobProgress = PrintJobProgress(
                            state = PrintJobState.FAILED,
                            errorMessage = e.message ?: "Network error while streaming print job."
                        )
                    )
                }
            }
        }
    }
}
