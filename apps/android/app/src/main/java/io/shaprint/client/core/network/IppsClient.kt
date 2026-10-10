package io.shaprint.client.core.network

import io.shaprint.client.core.domain.model.PrintJobRequest
import io.shaprint.client.core.domain.model.SharedPrinter
import io.shaprint.client.core.domain.model.TrustedServer
import io.shaprint.client.core.ipp.IppMessage
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.withContext
import java.io.ByteArrayOutputStream
import java.io.InputStream
import java.io.OutputStream
import java.net.HttpURLConnection
import java.net.URL
import java.security.SecureRandom
import java.security.cert.CertificateException
import java.security.cert.X509Certificate
import javax.net.ssl.HttpsURLConnection
import javax.net.ssl.SSLContext
import javax.net.ssl.SSLException
import javax.net.ssl.TrustManager
import javax.net.ssl.X509TrustManager

interface IppsClient {
    suspend fun queryPrinters(server: TrustedServer): List<SharedPrinter>

    suspend fun submitPrintJob(
        server: TrustedServer,
        jobRequest: PrintJobRequest,
        streamRasterPayload: (OutputStream) -> Unit
    ): IppMessage.ParsedJobSubmissionResult
}

class DefaultIppsClient : IppsClient {

    override suspend fun queryPrinters(server: TrustedServer): List<SharedPrinter> = withContext(Dispatchers.IO) {
        val printerUri = "ipp://${server.canonicalAddress}/ipp/print"
        val requestBytes = IppMessage.buildGetPrintersRequest(
            printerUri = printerUri,
            networkChannel = server.networkChannel
        )

        val targetUrl = "https://${server.canonicalAddress}/ipp/print"
        val responseBytes = postIpp(targetUrl, server.sha256Fingerprint) { os ->
            os.write(requestBytes)
        }

        val parsed = IppMessage.parseResponse(responseBytes, server.canonicalAddress)
        parsed.printers
    }

    override suspend fun submitPrintJob(
        server: TrustedServer,
        jobRequest: PrintJobRequest,
        streamRasterPayload: (OutputStream) -> Unit
    ): IppMessage.ParsedJobSubmissionResult = withContext(Dispatchers.IO) {
        val headerBytes = IppMessage.buildPrintJobHeader(
            jobRequest = jobRequest,
            networkChannel = server.networkChannel
        )

        val targetUrl = "https://${server.canonicalAddress}/ipp/print/${jobRequest.printerName}"
        val responseBytes = postIpp(targetUrl, server.sha256Fingerprint, chunked = true) { os ->
            os.write(headerBytes)
            streamRasterPayload(os)
        }

        IppMessage.parseJobResponse(responseBytes)
    }

    private fun postIpp(
        urlStr: String,
        expectedFingerprint: String,
        chunked: Boolean = false,
        writeBody: (OutputStream) -> Unit
    ): ByteArray {
        val url = URL(urlStr)
        val connection = url.openConnection() as HttpsURLConnection

        // Configure TLS with strict certificate fingerprint pinning (ADR 0003, ADR 0016)
        val pinningTrustManager = object : X509TrustManager {
            override fun checkServerTrusted(chain: Array<X509Certificate>?, authType: String?) {
                val cert = chain?.firstOrNull()
                    ?: throw CertificateException("No server certificate presented by $urlStr")

                val actualFingerprint = TlsFingerprint.computeSha256(cert)
                if (!TlsFingerprint.matches(actualFingerprint, expectedFingerprint)) {
                    throw SSLException(
                        "Certificate fingerprint mismatch! Expected $expectedFingerprint but got $actualFingerprint"
                    )
                }
            }

            override fun checkClientTrusted(chain: Array<X509Certificate>?, authType: String?) {}

            override fun getAcceptedIssuers(): Array<X509Certificate> = emptyArray()
        }

        val sslContext = SSLContext.getInstance("TLS")
        sslContext.init(null, arrayOf<TrustManager>(pinningTrustManager), SecureRandom())
        connection.sslSocketFactory = sslContext.socketFactory
        connection.hostnameVerifier = javax.net.ssl.HostnameVerifier { _, _ -> true } // Identity is strictly pinned by SHA-256 fingerprint

        connection.requestMethod = "POST"
        connection.doOutput = true
        connection.connectTimeout = 10000
        connection.readTimeout = 30000
        if (chunked) {
            connection.setChunkedStreamingMode(65536)
        }
        connection.setRequestProperty("Content-Type", "application/ipp")
        connection.setRequestProperty("User-Agent", "ShaPrint-Android/1.0")

        connection.outputStream.use { os: OutputStream ->
            writeBody(os)
            os.flush()
        }

        val responseCode = connection.responseCode
        if (responseCode !in 200..299) {
            throw Exception("IPPS request failed with HTTP status $responseCode")
        }

        val inputStream: InputStream = connection.inputStream
        val baos = ByteArrayOutputStream()
        val buffer = ByteArray(4096)
        var bytesRead: Int
        while (inputStream.read(buffer).also { bytesRead = it } != -1) {
            baos.write(buffer, 0, bytesRead)
        }

        return baos.toByteArray()
    }
}
