package io.shaprint.client.core.security

import io.shaprint.client.core.domain.model.TrustedServer

/**
 * Contract for persisting and retrieving trusted servers and their Network Channels (ADR 0003, ADR 0016).
 */
interface TrustStore {
    suspend fun getTrustedServers(): List<TrustedServer>
    suspend fun findByCanonicalAddress(canonicalAddress: String): TrustedServer?
    suspend fun saveTrustedServer(server: TrustedServer)
    suspend fun removeTrustedServer(canonicalAddress: String)
}

/**
 * In-memory fallback / test double for TrustStore.
 */
class InMemoryTrustStore : TrustStore {
    private val servers = mutableMapOf<String, TrustedServer>()

    override suspend fun getTrustedServers(): List<TrustedServer> = servers.values.toList()

    override suspend fun findByCanonicalAddress(canonicalAddress: String): TrustedServer? =
        servers[canonicalAddress]

    override suspend fun saveTrustedServer(server: TrustedServer) {
        servers[server.canonicalAddress] = server
    }

    override suspend fun removeTrustedServer(canonicalAddress: String) {
        servers.remove(canonicalAddress)
    }
}
