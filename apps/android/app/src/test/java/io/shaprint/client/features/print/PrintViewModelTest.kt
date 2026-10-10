package io.shaprint.client.features.print

import io.shaprint.client.core.domain.model.DuplexMode
import io.shaprint.client.core.domain.model.PrintColorMode
import io.shaprint.client.core.domain.model.PrintJobRequest
import io.shaprint.client.core.domain.model.PrintJobState
import io.shaprint.client.core.domain.model.RequestedMedia
import io.shaprint.client.core.domain.model.SharedPrinter
import io.shaprint.client.core.domain.model.TrustedServer
import io.shaprint.client.core.ipp.IppConstants
import io.shaprint.client.core.ipp.IppMessage
import io.shaprint.client.core.network.IppsClient
import io.shaprint.client.core.security.InMemoryTrustStore
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.ExperimentalCoroutinesApi
import kotlinx.coroutines.runBlocking
import kotlinx.coroutines.test.StandardTestDispatcher
import kotlinx.coroutines.test.resetMain
import kotlinx.coroutines.test.setMain
import org.junit.After
import org.junit.Assert.*
import org.junit.Before
import org.junit.Test
import java.io.OutputStream

@OptIn(ExperimentalCoroutinesApi::class)
class PrintViewModelTest {

    private val testDispatcher = StandardTestDispatcher()
    private val trustStore = InMemoryTrustStore()

    private class FakeIppsClient(
        var jobResult: IppMessage.ParsedJobSubmissionResult = IppMessage.ParsedJobSubmissionResult(
            statusCode = IppConstants.STATUS_OK,
            isSuccess = true
        )
    ) : IppsClient {
        var lastJobRequest: PrintJobRequest? = null
        var streamedBytes: ByteArray = ByteArray(0)

        override suspend fun queryPrinters(server: TrustedServer): List<SharedPrinter> = emptyList()

        override suspend fun submitPrintJob(
            server: TrustedServer,
            jobRequest: PrintJobRequest,
            streamRasterPayload: (OutputStream) -> Unit
        ): IppMessage.ParsedJobSubmissionResult {
            lastJobRequest = jobRequest
            val baos = java.io.ByteArrayOutputStream()
            streamRasterPayload(baos)
            streamedBytes = baos.toByteArray()
            return jobResult
        }
    }

    @Before
    fun setUp() {
        Dispatchers.setMain(testDispatcher)
    }

    @After
    fun tearDown() {
        Dispatchers.resetMain()
    }

    @Test
    fun submitPrintJobStreamsPayloadAndTransitionsToCompleted() = runBlocking {
        val server = TrustedServer(
            host = "192.168.1.50",
            port = 48631,
            computerName = "PRINT-SERVER",
            sha256Fingerprint = "AA:BB:CC",
            networkChannel = "channel-123"
        )
        trustStore.saveTrustedServer(server)

        val client = FakeIppsClient()
        val viewModel = PrintViewModel(trustStore, client)

        val printer = SharedPrinter(
            name = "Epson L3210",
            serverCanonicalAddress = "192.168.1.50:48631"
        )
        viewModel.setPrinter(printer)
        viewModel.selectDocument("Invoice-2026.pdf", "content://docs/1", pageCount = 3)
        viewModel.updateMedia(RequestedMedia.OM_FOLIO)
        viewModel.updateColorMode(PrintColorMode.MONOCHROME)
        viewModel.updateCopies(2)

        viewModel.submitPrintJob { out ->
            out.write("RaS2-mock-payload".toByteArray(Charsets.UTF_8))
        }
        testDispatcher.scheduler.advanceUntilIdle()

        val state = viewModel.uiState.value
        assertEquals(PrintJobState.COMPLETED, state.jobProgress.state)
        assertEquals(3, state.jobProgress.totalPages)

        val submitted = client.lastJobRequest
        assertNotNull(submitted)
        assertEquals("Invoice-2026.pdf", submitted?.jobName)
        assertEquals(RequestedMedia.OM_FOLIO, submitted?.media)
        assertEquals(PrintColorMode.MONOCHROME, submitted?.colorMode)
        assertEquals(2, submitted?.copies)
        assertEquals("RaS2-mock-payload", String(client.streamedBytes, Charsets.UTF_8))
    }

    @Test
    fun submitPrintJobSurfacesUnauthorizedChannelError() = runBlocking {
        val server = TrustedServer(
            host = "192.168.1.50",
            port = 48631,
            computerName = "PRINT-SERVER",
            sha256Fingerprint = "AA:BB:CC"
        )
        trustStore.saveTrustedServer(server)

        val client = FakeIppsClient(
            IppMessage.ParsedJobSubmissionResult(
                statusCode = IppConstants.STATUS_CLIENT_ERROR_NOT_AUTHORIZED,
                isSuccess = false,
                errorMessage = "Unauthorized: Invalid or missing Network Channel secret (0x0403)."
            )
        )
        val viewModel = PrintViewModel(trustStore, client)
        viewModel.setPrinter(SharedPrinter("Epson", "192.168.1.50:48631"))

        viewModel.submitPrintJob { }
        testDispatcher.scheduler.advanceUntilIdle()

        val state = viewModel.uiState.value
        assertEquals(PrintJobState.FAILED, state.jobProgress.state)
        assertTrue(state.jobProgress.errorMessage?.contains("Network Channel") == true)
    }
}
