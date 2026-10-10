package io.shaprint.client.core.network

import org.junit.Assert.assertEquals
import org.junit.Test

class PortsTest {
    @Test
    fun dedicatedPortsMatchArchitecturalDecisions() {
        // ADR 0003 & ADR 0014
        assertEquals(48631, Ports.IPPS_SERVER_DEFAULT_PORT)
        // ADR 0005 & ADR 0014
        assertEquals(48632, Ports.CLIENT_PROXY_DEFAULT_PORT)
        // ADR 0004 & ADR 0014
        assertEquals(48633, Ports.DISCOVERY_DEFAULT_PORT)
        assertEquals("_shaprint-ipps._tcp.local.", Ports.MDNS_SERVICE_TYPE)
    }
}
