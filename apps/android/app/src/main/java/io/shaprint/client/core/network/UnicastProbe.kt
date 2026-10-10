package io.shaprint.client.core.network

import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.withContext
import java.net.InetSocketAddress
import java.net.Socket

interface UnicastProbe {
    suspend fun probe(host: String, port: Int, timeoutMs: Int = 2000): Boolean
}

class DefaultUnicastProbe : UnicastProbe {
    override suspend fun probe(host: String, port: Int, timeoutMs: Int): Boolean = withContext(Dispatchers.IO) {
        try {
            Socket().use { socket ->
                socket.connect(InetSocketAddress(host, port), timeoutMs)
                true
            }
        } catch (_: Exception) {
            false
        }
    }
}

class FakeUnicastProbe(private val reachableAddresses: Set<String> = emptySet()) : UnicastProbe {
    override suspend fun probe(host: String, port: Int, timeoutMs: Int): Boolean {
        return reachableAddresses.contains("$host:$port") || reachableAddresses.contains(host)
    }
}
