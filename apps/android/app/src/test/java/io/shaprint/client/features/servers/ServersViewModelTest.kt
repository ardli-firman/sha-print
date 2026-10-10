package io.shaprint.client.features.servers

import io.shaprint.client.core.domain.model.NearbyServer
import io.shaprint.client.core.domain.model.TrustedServer
import io.shaprint.client.core.network.CertificateInspectionResult
import io.shaprint.client.core.network.TlsCertificateInspector
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
class ServersViewModelTest {

    private val testDispatcher = StandardTestDispatcher()
    private val trustStore = InMemoryTrustStore()

    private class FakeTlsInspector(
        var result: CertificateInspectionResult = CertificateInspectionResult.Success(
            sha256Fingerprint = "AA:BB:CC:DD:EE:FF",
            subject = "CN=ShaPrint Server",
            issuer = "CN=ShaPrint Server",
            rawDerBytes = byteArrayOf(1, 2, 3)
        )
    ) : TlsCertificateInspector {
        override suspend fun inspectCertificate(host: String, port: Int, timeoutMs: Int): CertificateInspectionResult {
            return result
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
    fun loadTrustedServersReadsFromTrustStore() = runBlocking {
        val existing = TrustedServer(
            host = "192.168.1.100",
            port = 48631,
            computerName = "OFFICE-PC",
            sha256Fingerprint = "11:22:33:44"
        )
        trustStore.saveTrustedServer(existing)

        val viewModel = ServersViewModel(trustStore, FakeTlsInspector())
        testDispatcher.scheduler.advanceUntilIdle()

        val state = viewModel.uiState.value
        assertEquals(1, state.trustedServers.size)
        assertEquals("OFFICE-PC", state.trustedServers.first().computerName)
    }

    @Test
    fun reviewServerInspectsFingerprintAndApprovesSuccessfully() = runBlocking {
        val inspector = FakeTlsInspector()
        val viewModel = ServersViewModel(trustStore, inspector)
        testDispatcher.scheduler.advanceUntilIdle()

        val nearby = NearbyServer(
            host = "192.168.1.200",
            port = 48631,
            computerName = "NEW-SERVER",
            advertisedQueues = listOf("Epson-L3210")
        )

        viewModel.reviewServer(nearby)
        testDispatcher.scheduler.advanceUntilIdle()

        var state = viewModel.uiState.value
        assertNotNull(state.pendingReviewServer)
        assertEquals("AA:BB:CC:DD:EE:FF", state.inspectedFingerprint)
        assertFalse(state.inspectingFingerprint)
        assertNull(state.securityAlert)

        viewModel.updateChannelInput("my-channel-secret")
        viewModel.approvePendingServer()
        testDispatcher.scheduler.advanceUntilIdle()

        state = viewModel.uiState.value
        assertNull(state.pendingReviewServer)
        assertEquals(1, state.trustedServers.size)
        val trusted = state.trustedServers.first()
        assertEquals("NEW-SERVER", trusted.computerName)
        assertEquals("AA:BB:CC:DD:EE:FF", trusted.sha256Fingerprint)
        assertEquals("my-channel-secret", trusted.networkChannel)
    }

    @Test
    fun reviewServerTriggersSecurityWarningWhenFingerprintChanged() = runBlocking {
        // First trust with fingerprint A
        val initialServer = TrustedServer(
            host = "192.168.1.50",
            port = 48631,
            computerName = "PRINT-HOST",
            sha256Fingerprint = "ORIGINAL:FINGERPRINT:11:22"
        )
        trustStore.saveTrustedServer(initialServer)

        // Live inspector presents DIFFERENT fingerprint B
        val inspector = FakeTlsInspector(
            CertificateInspectionResult.Success(
                sha256Fingerprint = "DIFFERENT:FINGERPRINT:99:88",
                subject = "CN=Rogue Server",
                issuer = "CN=Rogue Server",
                rawDerBytes = byteArrayOf(9, 9)
            )
        )
        val viewModel = ServersViewModel(trustStore, inspector)
        testDispatcher.scheduler.advanceUntilIdle()

        val nearby = NearbyServer(
            host = "192.168.1.50",
            port = 48631,
            computerName = "PRINT-HOST"
        )
        viewModel.reviewServer(nearby)
        testDispatcher.scheduler.advanceUntilIdle()

        val state = viewModel.uiState.value
        assertNotNull(state.securityAlert)
        assertTrue(state.securityAlert?.contains("SECURITY WARNING") == true)
        assertTrue(state.securityAlert?.contains("CHANGED") == true)
    }

    @Test
    fun forgetServerRemovesFromStoreAndState() = runBlocking {
        val server = TrustedServer(
            host = "10.0.0.1",
            port = 48631,
            computerName = "TEMP",
            sha256Fingerprint = "FF:EE:DD"
        )
        trustStore.saveTrustedServer(server)

        val viewModel = ServersViewModel(trustStore, FakeTlsInspector())
        testDispatcher.scheduler.advanceUntilIdle()
        assertEquals(1, viewModel.uiState.value.trustedServers.size)

        viewModel.forgetServer("10.0.0.1:48631")
        testDispatcher.scheduler.advanceUntilIdle()

        assertEquals(0, viewModel.uiState.value.trustedServers.size)
        assertNull(trustStore.findByCanonicalAddress("10.0.0.1:48631"))
    }
}
