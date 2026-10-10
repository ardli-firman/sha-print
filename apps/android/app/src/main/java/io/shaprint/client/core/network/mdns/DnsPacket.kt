package io.shaprint.client.core.network.mdns

import java.io.ByteArrayOutputStream
import java.io.DataOutputStream
import java.net.InetAddress
import java.nio.ByteBuffer

/**
 * Lightweight DNS-SD packet encoder and decoder for ShaPrint multicast discovery (RFC 1035, RFC 6762, RFC 6763).
 */
object DnsPacket {

    const val TYPE_A = 1
    const val TYPE_PTR = 12
    const val TYPE_TXT = 16
    const val TYPE_AAAA = 28
    const val TYPE_SRV = 33

    const val CLASS_IN = 1

    /**
     * Builds a standard DNS PTR query datagram for `_shaprint-ipps._tcp.local.`.
     */
    fun buildQuery(serviceType: String = "_shaprint-ipps._tcp.local."): ByteArray {
        val baos = ByteArrayOutputStream()
        val dos = DataOutputStream(baos)

        // Header: ID=0, Flags=0x0000 (standard query), QDCOUNT=1, ANCOUNT=0, NSCOUNT=0, ARCOUNT=0
        dos.writeShort(0x0000)
        dos.writeShort(0x0000)
        dos.writeShort(1)
        dos.writeShort(0)
        dos.writeShort(0)
        dos.writeShort(0)

        // Question: QNAME
        writeDomainName(dos, serviceType)
        // QTYPE: PTR
        dos.writeShort(TYPE_PTR)
        // QCLASS: IN
        dos.writeShort(CLASS_IN)

        return baos.toByteArray()
    }

    private fun writeDomainName(dos: DataOutputStream, domain: String) {
        val labels = domain.trimEnd('.').split('.')
        for (label in labels) {
            val bytes = label.toByteArray(Charsets.UTF_8)
            dos.writeByte(bytes.size)
            dos.write(bytes)
        }
        dos.writeByte(0)
    }

    data class DiscoveredService(
        val instanceName: String,
        val host: String,
        val port: Int,
        val computerName: String,
        val advertisedQueues: List<String>,
        val isGoodbye: Boolean = false
    )

    /**
     * Parses a DNS response datagram into DiscoveredService if it matches ShaPrint records.
     */
    fun parseResponse(data: ByteArray, length: Int, senderAddress: InetAddress): DiscoveredService? {
        if (length < 12) return null
        val buffer = ByteBuffer.wrap(data, 0, length)

        val id = buffer.short
        val flags = buffer.short.toInt() and 0xFFFF
        val isResponse = (flags and 0x8000) != 0
        if (!isResponse) return null

        val qdCount = buffer.short.toInt() and 0xFFFF
        val anCount = buffer.short.toInt() and 0xFFFF
        val nsCount = buffer.short.toInt() and 0xFFFF
        val arCount = buffer.short.toInt() and 0xFFFF

        // Skip Questions
        for (i in 0 until qdCount) {
            readDomainName(buffer, data) ?: return null
            if (buffer.remaining() < 4) return null
            buffer.short // qtype
            buffer.short // qclass
        }

        val totalRecords = anCount + nsCount + arCount
        var ptrInstance: String? = null
        var srvPort: Int? = null
        var srvTarget: String? = null
        var resolvedIp: String? = null
        val txtKeyValues = mutableMapOf<String, String>()
        val advertisedQueues = mutableListOf<String>()
        var isGoodbye = false

        for (i in 0 until totalRecords) {
            if (!buffer.hasRemaining()) break
            val name = readDomainName(buffer, data) ?: break
            if (buffer.remaining() < 10) break
            val type = buffer.short.toInt() and 0xFFFF
            val recordClass = buffer.short.toInt() and 0x7FFF
            val ttl = buffer.int.toLong() and 0xFFFFFFFFL
            val rdLength = buffer.short.toInt() and 0xFFFF

            if (ttl == 0L) {
                isGoodbye = true
            }

            if (buffer.remaining() < rdLength) break
            val rdataStart = buffer.position()

            when (type) {
                TYPE_PTR -> {
                    ptrInstance = readDomainName(buffer, data)
                }
                TYPE_SRV -> {
                    if (rdLength >= 6) {
                        buffer.short // priority
                        buffer.short // weight
                        srvPort = buffer.short.toInt() and 0xFFFF
                        srvTarget = readDomainName(buffer, data)
                    }
                }
                TYPE_TXT -> {
                    var remainingTxt = rdLength
                    while (remainingTxt > 0 && buffer.hasRemaining()) {
                        val len = buffer.get().toInt() and 0xFF
                        remainingTxt--
                        if (len > 0 && len <= remainingTxt && buffer.remaining() >= len) {
                            val strBytes = ByteArray(len)
                            buffer.get(strBytes)
                            remainingTxt -= len
                            val entry = String(strBytes, Charsets.UTF_8)
                            val eqIdx = entry.indexOf('=')
                            if (eqIdx > 0) {
                                val key = entry.substring(0, eqIdx)
                                val value = entry.substring(eqIdx + 1)
                                txtKeyValues[key] = value
                                if (key.equals("queue", ignoreCase = true)) {
                                    advertisedQueues.add(value)
                                }
                            }
                        } else {
                            break
                        }
                    }
                }
                TYPE_A -> {
                    if (rdLength == 4) {
                        val ipBytes = ByteArray(4)
                        buffer.get(ipBytes)
                        resolvedIp = InetAddress.getByAddress(ipBytes).hostAddress
                    }
                }
            }

            // Seek to exact end of RDATA in case parser didn't consume all
            buffer.position(rdataStart + rdLength)
        }

        val effectivePort = srvPort ?: 48631
        val effectiveHost = resolvedIp ?: senderAddress.hostAddress ?: "127.0.0.1"
        val computerName = txtKeyValues["name"]
            ?: ptrInstance?.split("._")?.firstOrNull()
            ?: effectiveHost

        return DiscoveredService(
            instanceName = ptrInstance ?: computerName,
            host = effectiveHost,
            port = effectivePort,
            computerName = computerName,
            advertisedQueues = advertisedQueues,
            isGoodbye = isGoodbye
        )
    }

    private fun readDomainName(buffer: ByteBuffer, fullData: ByteArray): String? {
        val sb = StringBuilder()
        var jumped = false
        var savedPos = -1
        var steps = 0

        while (buffer.hasRemaining() && steps < 128) {
            steps++
            val len = buffer.get().toInt() and 0xFF
            if (len == 0) {
                break
            }
            if ((len and 0xC0) == 0xC0) {
                // Pointer compression
                if (!buffer.hasRemaining()) return null
                val b2 = buffer.get().toInt() and 0xFF
                val offset = ((len and 0x3F) shl 8) or b2
                if (!jumped) {
                    savedPos = buffer.position()
                    jumped = true
                }
                if (offset >= fullData.size) return null
                buffer.position(offset)
            } else {
                if (buffer.remaining() < len) return null
                val labelBytes = ByteArray(len)
                buffer.get(labelBytes)
                if (sb.isNotEmpty()) sb.append('.')
                sb.append(String(labelBytes, Charsets.UTF_8))
            }
        }

        if (jumped && savedPos != -1) {
            buffer.position(savedPos)
        }

        return if (sb.isNotEmpty()) sb.toString() else null
    }
}
