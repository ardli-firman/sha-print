package io.shaprint.client.features.discovery

import io.shaprint.client.core.domain.model.NearbyServer
import io.shaprint.client.core.network.FakeUnicastProbe
import io.shaprint.client.core.network.mdns.MdnsScanner
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.ExperimentalCoroutinesApi
import kotlinx.coroutines.flow.Flow
import kotlinx.coroutines.flow.flowOf
import kotlinx.coroutines.runBlocking
import kotlinx.coroutines.test.StandardTestDispatcher
import kotlinx.coroutines.test.resetMain
import kotlinx.coroutines.test.setMain
import org.junit.After
import org.junit.Assert.*
import org.junit.Before
import org.junit.Test

@OptIn(ExperimentalCoroutinesApi::class)
class DiscoveryViewModelTest {

    private val testDispatcher = StandardTestDispatcher()

    private class FakeMdnsScanner(private val servers: List<NearbyServer>) : MdnsScanner {
        override fun scan(timeoutMs: Long): Flow<NearbyServer> = flowOf(*servers.toTypedArray())
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
    fun startScanDiscoversServersAndUpdatesState() = runBlocking {
        val discoveredServer = NearbyServer(
            host = "192.168.1.100",
            port = 48631,
            computerName = "OFFICE-PC",
            advertisedQueues = listOf("Epson-L3210")
        )
        val scanner = FakeMdnsScanner(listOf(discoveredServer))
        val probe = FakeUnicastProbe(setOf("192.168.1.100:48631"))
        val viewModel = DiscoveryViewModel(scanner, probe)

        viewModel.startScan(timeoutMs = 1000)
        testDispatcher.scheduler.advanceUntilIdle()

        val state = viewModel.uiState.value
        assertFalse(state.isScanning)
        assertEquals(1, state.nearbyServers.size)
        assertEquals("OFFICE-PC", state.nearbyServers.first().computerName)
        assertEquals("192.168.1.100:48631", state.nearbyServers.first().canonicalAddress)
    }

    @Test
    fun manualServerValidationRejectsBlankHost() = runBlocking {
        val scanner = FakeMdnsScanner(emptyList())
        val probe = FakeUnicastProbe()
        val viewModel = DiscoveryViewModel(scanner, probe)

        viewModel.openManualServerDialog()
        viewModel.updateManualHost("   ")
        viewModel.submitManualServer()
        testDispatcher.scheduler.advanceUntilIdle()

        val state = viewModel.uiState.value
        assertTrue(state.showManualServerDialog)
        assertNotNull(state.manualInputError)
        assertEquals(0, state.nearbyServers.size)
    }

    @Test
    fun manualServerValidationRejectsInvalidPort() = runBlocking {
        val scanner = FakeMdnsScanner(emptyList())
        val probe = FakeUnicastProbe()
        val viewModel = DiscoveryViewModel(scanner, probe)

        viewModel.openManualServerDialog()
        viewModel.updateManualHost("10.0.0.5")
        viewModel.updateManualPort("99999") // Out of range
        viewModel.submitManualServer()
        testDispatcher.scheduler.advanceUntilIdle()

        val state = viewModel.uiState.value
        assertTrue(state.showManualServerDialog)
        assertNotNull(state.manualInputError)
    }

    @Test
    fun manualServerValidInputAddsServerWithProbeResult() = runBlocking {
        val scanner = FakeMdnsScanner(emptyList())
        val probe = FakeUnicastProbe(setOf("10.0.0.5:48631"))
        val viewModel = DiscoveryViewModel(scanner, probe)

        viewModel.openManualServerDialog()
        viewModel.updateManualHost("10.0.0.5")
        viewModel.updateManualPort("48631")
        viewModel.submitManualServer()
        testDispatcher.scheduler.advanceUntilIdle()

        val state = viewModel.uiState.value
        assertFalse(state.showManualServerDialog)
        assertNull(state.manualInputError)
        assertEquals(1, state.nearbyServers.size)
        val added = state.nearbyServers.first()
        assertEquals("10.0.0.5", added.host)
        assertEquals(48631, added.port)
        assertTrue(added.isOnline)
    }

    @Test
    fun checkReachabilityUpdatesOnlineStatus() = runBlocking {
        val server = NearbyServer(
            host = "10.0.0.9",
            port = 48631,
            computerName = "TEST",
            isOnline = false
        )
        val scanner = FakeMdnsScanner(listOf(server))
        val probe = FakeUnicastProbe(setOf("10.0.0.9:48631"))
        val viewModel = DiscoveryViewModel(scanner, probe)

        viewModel.startScan()
        testDispatcher.scheduler.advanceUntilIdle()

        // Probe marks online
        viewModel.checkReachability(server)
        testDispatcher.scheduler.advanceUntilIdle()

        val updated = viewModel.uiState.value.nearbyServers.first()
        assertTrue(updated.isOnline)
    }
}
