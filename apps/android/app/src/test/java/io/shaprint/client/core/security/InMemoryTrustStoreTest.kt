package io.shaprint.client.core.security

import io.shaprint.client.core.domain.model.TrustedServer
import kotlinx.coroutines.runBlocking
import org.junit.Assert.*
import org.junit.Test

class InMemoryTrustStoreTest {

    @Test
    fun saveAndRetrieveTrustedServer() = runBlocking {
        val store = InMemoryTrustStore()
        val server = TrustedServer(
            host = "192.168.1.100",
            port = 48631,
            computerName = "DESKTOP-PRINT",
            sha256Fingerprint = "11:22:33:44",
            networkChannel = "my-secret-channel"
        )

        store.saveTrustedServer(server)

        val retrieved = store.findByCanonicalAddress("192.168.1.100:48631")
        assertNotNull(retrieved)
        assertEquals("DESKTOP-PRINT", retrieved?.computerName)
        assertEquals("my-secret-channel", retrieved?.networkChannel)

        val all = store.getTrustedServers()
        assertEquals(1, all.size)

        store.removeTrustedServer("192.168.1.100:48631")
        assertNull(store.findByCanonicalAddress("192.168.1.100:48631"))
        assertTrue(store.getTrustedServers().isEmpty())
    }
}
