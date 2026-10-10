package io.shaprint.client.core.domain.model

/**
 * A server a client has seen advertise itself on the local network (see CONTEXT.md).
 * A nearby server is a hint to review, never a trusted server.
 */
data class NearbyServer(
    val host: String,
    val port: Int,
    val computerName: String,
    val advertisedQueues: List<String> = emptyList(),
    val isOnline: Boolean = true
) {
    val canonicalAddress: String
        get() = "$host:$port"
}

/**
 * A server whose certificate identity a client user has reviewed and approved for printer
 * discovery and print jobs (see CONTEXT.md).
 */
data class TrustedServer(
    val host: String,
    val port: Int,
    val computerName: String,
    val sha256Fingerprint: String,
    val networkChannel: String? = null,
    val isOnline: Boolean = true
) {
    val canonicalAddress: String
        get() = "$host:$port"
}
