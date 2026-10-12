package io.shaprint.client.core.network

import android.net.Uri
import io.shaprint.client.core.domain.model.PrintJobRequest
import io.shaprint.client.core.domain.model.SharedPrinter
import io.shaprint.client.core.domain.model.TrustedServer
import io.shaprint.client.core.ipp.IppConstants
import io.shaprint.client.core.ipp.IppMessage
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.withContext
import java.io.ByteArrayOutputStream
import java.io.File
import java.io.FileInputStream
import java.io.FileOutputStream
import java.io.IOException
import java.io.InputStream
import java.io.OutputStream
import java.net.URL
import java.security.SecureRandom
import java.security.cert.CertificateException
import java.security.cert.X509Certificate
import javax.net.ssl.HttpsURLConnection
import javax.net.ssl.SSLContext
import javax.net.ssl.TrustManager
import javax.net.ssl.X509TrustManager

interface IppsClient {
    suspend fun queryPrinters(server: TrustedServer): List<SharedPrinter>

    suspend fun queryPrinterAttributes(server: TrustedServer, printerName: String): SharedPrinter?

    suspend fun submitPrintJob(
        server: TrustedServer,
        jobRequest: PrintJobRequest,
        streamRasterPayload: (OutputStream) -> Unit
    ): IppMessage.ParsedJobSubmissionResult
}

class DefaultIppsClient : IppsClient {

    override suspend fun queryPrinters(server: TrustedServer): List<SharedPrinter> = withContext(Dispatchers.IO) {
        val printerUri = "ipps://${server.canonicalAddress}/ipp/print"
        val requestBytes = IppMessage.buildGetPrintersRequest(
            printerUri = printerUri,
            networkChannel = server.networkChannel,
            operationId = IppConstants.OP_CUPS_GET_PRINTERS
        )

        val targetUrl = "https://${server.canonicalAddress}/ipp/print"
        val responseBytes = postIpp(
            urlStr = targetUrl,
            expectedFingerprint = server.sha256Fingerprint,
            fixedLength = requestBytes.size.toLong()
        ) { os ->
            os.write(requestBytes)
        }

        val parsed = IppMessage.parseResponse(responseBytes, server.canonicalAddress)
        if (parsed.statusCode != IppConstants.STATUS_OK &&
            parsed.statusCode != IppConstants.STATUS_OK_IGNORED_OR_SUBSTITUTED
        ) {
            throw IOException("IPP query-printers failed with status: 0x${String.format("%04X", parsed.statusCode)}")
        }
        parsed.printers
    }

    override suspend fun queryPrinterAttributes(server: TrustedServer, printerName: String): SharedPrinter? =
        withContext(Dispatchers.IO) {
            val encodedName = PrintJobRequest.percentEncodeUriSegment(printerName)
            val printerUri = "ipps://${server.canonicalAddress}/ipp/print/$encodedName"
            val requestBytes = IppMessage.buildGetPrintersRequest(
                printerUri = printerUri,
                networkChannel = server.networkChannel,
                operationId = IppConstants.OP_GET_PRINTER_ATTRIBUTES
            )

            val targetUrl = "https://${server.canonicalAddress}/ipp/print/$encodedName"
            try {
                val responseBytes = postIpp(
                    urlStr = targetUrl,
                    expectedFingerprint = server.sha256Fingerprint,
                    fixedLength = requestBytes.size.toLong()
                ) { os ->
                    os.write(requestBytes)
                }
                val parsed = IppMessage.parseResponse(responseBytes, server.canonicalAddress)
                parsed.printers.firstOrNull() ?: SharedPrinter(
                    name = printerName,
                    serverCanonicalAddress = server.canonicalAddress,
                    isOnline = true
                )
            } catch (_: Exception) {
                null
            }
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

        val encodedPrinterName = try {
            Uri.encode(jobRequest.printerName) ?: jobRequest.encodedPrinterName
        } catch (_: Throwable) {
            jobRequest.encodedPrinterName
        }
        val targetUrl = "https://${server.canonicalAddress}/ipp/print/$encodedPrinterName"

        // Buffer the generated PWG-raster stream to a temporary file on disk.
        // This calculates exact Content-Length required by the ShaPrint desktop server,
        // eliminating Transfer-Encoding: chunked (which server rejects with 501, causing Broken Pipe)
        // while preserving minimal RAM footprint on mobile.
        val tempRasterFile = File.createTempFile("shaprint_raster_", ".pwg")
        try {
            FileOutputStream(tempRasterFile).use { fos ->
                streamRasterPayload(fos)
            }

            val totalContentLength = headerBytes.size.toLong() + tempRasterFile.length()

            val responseBytes = postIpp(
                urlStr = targetUrl,
                expectedFingerprint = server.sha256Fingerprint,
                fixedLength = totalContentLength
            ) { os ->
                os.write(headerBytes)
                FileInputStream(tempRasterFile).use { fis ->
                    val buffer = ByteArray(65536)
                    var bytesRead: Int
                    while (fis.read(buffer).also { bytesRead = it } != -1) {
                        os.write(buffer, 0, bytesRead)
                    }
                }
            }

            IppMessage.parseJobResponse(responseBytes)
        } finally {
            tempRasterFile.delete()
        }
    }

    private fun postIpp(
        urlStr: String,
        expectedFingerprint: String,
        fixedLength: Long? = null,
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
                    throw CertificateException(
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

        if (fixedLength != null) {
            connection.setFixedLengthStreamingMode(fixedLength)
        }

        connection.setRequestProperty("Content-Type", "application/ipp")
        connection.setRequestProperty("User-Agent", "ShaPrint-Android-Client/1.0")

        connection.outputStream.use { os ->
            writeBody(os)
            os.flush()
        }

        val responseCode = connection.responseCode
        val inputStream: InputStream = if (responseCode in 200..299) {
            connection.inputStream
        } else {
            val errorBytes = connection.errorStream?.readBytes() ?: ByteArray(0)
            val errorText = String(errorBytes, Charsets.UTF_8).trim()
            if (errorText.isNotEmpty() && !errorText.startsWith("\u0001") && !errorText.startsWith("\u0002")) {
                throw IOException("Server responded with HTTP $responseCode: $errorText")
            }
            java.io.ByteArrayInputStream(errorBytes)
        }

        val buffer = ByteArrayOutputStream()
        inputStream.use { input ->
            input.copyTo(buffer)
        }
        return buffer.toByteArray()
    }
}
