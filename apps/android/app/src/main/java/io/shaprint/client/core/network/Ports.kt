package io.shaprint.client.core.network

/**
 * Dedicated ports for ShaPrint networking (ADR 0003, ADR 0004, ADR 0014).
 */
object Ports {
    /** Dedicated port for the server IPPS sharing endpoint. */
    const val IPPS_SERVER_DEFAULT_PORT = 48631

    /** Dedicated loopback port for the desktop client proxy (reference only). */
    const val CLIENT_PROXY_DEFAULT_PORT = 48632

    /** Dedicated UDP port for multicast DNS discovery. */
    const val DISCOVERY_DEFAULT_PORT = 48633

    /** Multicast DNS service type for ShaPrint servers. */
    const val MDNS_SERVICE_TYPE = "_shaprint-ipps._tcp.local."
}
