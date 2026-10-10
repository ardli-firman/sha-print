package io.shaprint.client.features.printers

import io.shaprint.client.core.domain.model.SharedPrinter
import io.shaprint.client.core.domain.model.TrustedServer
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

@OptIn(ExperimentalCoroutinesApi::class)
class PrintersViewModelTest {

    private val testDispatcher = StandardTestDispatcher()
    private val trustStore = InMemoryTrustStore()

    private class FakeIppsClient(
        private val printersMap: Map<String, List<SharedPrinter>> = emptyMap()
    ) : IppsClient {
        override suspend fun queryPrinters(server: TrustedServer): List<SharedPrinter> {
            return printersMap[server.canonicalAddress] ?: emptyList()
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
    fun loadPrintersAggregatesPrintersFromAllTrustedServers() = runBlocking {
        val server1 = TrustedServer(
            host = "192.168.1.10",
            port = 48631,
            computerName = "SERVER-1",
            sha256Fingerprint = "AA:11"
        )
        val server2 = TrustedServer(
            host = "192.168.1.20",
            port = 48631,
            computerName = "SERVER-2",
            sha256Fingerprint = "BB:22"
        )
        trustStore.saveTrustedServer(server1)
        trustStore.saveTrustedServer(server2)

        val printer1 = SharedPrinter(name = "Epson L3210", serverCanonicalAddress = "192.168.1.10:48631")
        val printer2 = SharedPrinter(name = "HP LaserJet", serverCanonicalAddress = "192.168.1.20:48631")

        val client = FakeIppsClient(
            mapOf(
                "192.168.1.10:48631" to listOf(printer1),
                "192.168.1.20:48631" to listOf(printer2)
            )
        )

        val viewModel = PrintersViewModel(trustStore, client)
        testDispatcher.scheduler.advanceUntilIdle()

        val state = viewModel.uiState.value
        assertFalse(state.isLoading)
        assertEquals(2, state.printers.size)
        assertTrue(state.printers.any { it.name == "Epson L3210" })
        assertTrue(state.printers.any { it.name == "HP LaserJet" })
    }

    @Test
    fun selectPrinterUpdatesSelectedPrinterState() = runBlocking {
        val viewModel = PrintersViewModel(trustStore, FakeIppsClient())
        testDispatcher.scheduler.advanceUntilIdle()

        val printer = SharedPrinter(name = "Canon Pixma", serverCanonicalAddress = "10.0.0.1:48631")
        viewModel.selectPrinter(printer)

        assertEquals("Canon Pixma", viewModel.uiState.value.selectedPrinter?.name)
    }
}
