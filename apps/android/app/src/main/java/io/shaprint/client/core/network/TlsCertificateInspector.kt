package io.shaprint.client.core.network

import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.withContext
import java.net.InetSocketAddress
import java.security.MessageDigest
import java.security.SecureRandom
import java.security.cert.X509Certificate
import java.util.Locale
import javax.net.ssl.SSLContext
import javax.net.ssl.SSLSocket
import javax.net.ssl.TrustManager
import javax.net.ssl.X509TrustManager

sealed class CertificateInspectionResult {
    data class Success(
        val sha256Fingerprint: String,
        val subject: String,
        val issuer: String,
        val rawDerBytes: ByteArray
    ) : CertificateInspectionResult() {
        override fun equals(other: Any?): Boolean {
            if (this === other) return true
            if (other !is Success) return false
            return sha256Fingerprint == other.sha256Fingerprint && subject == other.subject
        }

        override fun hashCode(): Int = sha256Fingerprint.hashCode()
    }

    data class Failure(val reason: String) : CertificateInspectionResult()
}

interface TlsCertificateInspector {
    suspend fun inspectCertificate(host: String, port: Int, timeoutMs: Int = 5000): CertificateInspectionResult
}

object TlsFingerprint {
    /**
     * Computes uppercase colon-delimited SHA-256 hex string from raw DER bytes.
     */
    fun computeSha256(derBytes: ByteArray): String {
        val digest = MessageDigest.getInstance("SHA-256").digest(derBytes)
        return formatHexColon(digest)
    }

    fun computeSha256(certificate: X509Certificate): String {
        return computeSha256(certificate.encoded)
    }

    fun normalizeFingerprint(fingerprint: String): String {
        return fingerprint.replace(":", "").replace(" ", "").uppercase(Locale.US)
    }

    fun matches(fingerprintA: String, fingerprintB: String): Boolean {
        return normalizeFingerprint(fingerprintA) == normalizeFingerprint(fingerprintB)
    }

    private fun formatHexColon(bytes: ByteArray): String {
        return bytes.joinToString(":") { String.format("%02X", it) }
    }
}

class DefaultTlsCertificateInspector : TlsCertificateInspector {

    override suspend fun inspectCertificate(host: String, port: Int, timeoutMs: Int): CertificateInspectionResult =
        withContext(Dispatchers.IO) {
            try {
                var capturedChain: Array<X509Certificate>? = null

                val capturingTrustManager = object : X509TrustManager {
                    override fun checkServerTrusted(chain: Array<X509Certificate>?, authType: String?) {
                        capturedChain = chain
                    }

                    override fun checkClientTrusted(chain: Array<X509Certificate>?, authType: String?) {}

                    override fun getAcceptedIssuers(): Array<X509Certificate> = emptyArray()
                }

                val sslContext = SSLContext.getInstance("TLS")
                sslContext.init(null, arrayOf<TrustManager>(capturingTrustManager), SecureRandom())

                val socketFactory = sslContext.socketFactory
                val socket = socketFactory.createSocket() as SSLSocket

                socket.use { sslSocket ->
                    sslSocket.connect(InetSocketAddress(host, port), timeoutMs)
                    sslSocket.soTimeout = timeoutMs
                    sslSocket.startHandshake()

                    val cert = capturedChain?.firstOrNull()
                        ?: sslSocket.session.peerCertificates.firstOrNull() as? X509Certificate
                        ?: return@withContext CertificateInspectionResult.Failure("No server certificate presented by $host:$port")

                    val fingerprint = TlsFingerprint.computeSha256(cert)
                    CertificateInspectionResult.Success(
                        sha256Fingerprint = fingerprint,
                        subject = cert.subjectX500Principal.name,
                        issuer = cert.issuerX500Principal.name,
                        rawDerBytes = cert.encoded
                    )
                }
            } catch (e: Exception) {
                CertificateInspectionResult.Failure(e.message ?: "Failed to inspect TLS certificate on $host:$port")
            }
        }
}
