package io.shaprint.client.core.network.mdns

import org.junit.Assert.*
import org.junit.Test
import java.io.ByteArrayOutputStream
import java.io.DataOutputStream
import java.net.InetAddress

class DnsPacketTest {

    @Test
    fun queryPacketEncodesCorrectHeaderAndQuestion() {
        val bytes = DnsPacket.buildQuery("_shaprint-ipps._tcp.local.")
        assertTrue(bytes.size >= 12)

        // Header checks
        assertEquals(0x00, bytes[0].toInt()) // ID high byte
        assertEquals(0x00, bytes[1].toInt()) // ID low byte
        assertEquals(0x00, bytes[2].toInt()) // Flags high
        assertEquals(0x00, bytes[3].toInt()) // Flags low
        assertEquals(0, bytes[4].toInt())    // QDCOUNT high
        assertEquals(1, bytes[5].toInt())    // QDCOUNT low = 1

        // QNAME should contain _shaprint-ipps (14), _tcp (4), local (5), 0
        assertEquals(14, bytes[12].toInt())
    }

    @Test
    fun parsesRealisticDnsResponseWithSrvTxtAndARecords() {
        val baos = ByteArrayOutputStream()
        val dos = DataOutputStream(baos)

        // Header: Response flag (0x8400), 0 questions, 1 answer, 0 auth, 2 additional
        dos.writeShort(0x0000)
        dos.writeShort(0x8400) // QR=1, AA=1
        dos.writeShort(0)      // QDCOUNT
        dos.writeShort(1)      // ANCOUNT (PTR)
        dos.writeShort(0)      // NSCOUNT
        dos.writeShort(2)      // ARCOUNT (SRV, TXT)

        // Answer 1: PTR
        writeDomainName(dos, "_shaprint-ipps._tcp.local")
        dos.writeShort(DnsPacket.TYPE_PTR)
        dos.writeShort(DnsPacket.CLASS_IN)
        dos.writeInt(120) // TTL
        val ptrTarget = encodeDomainName("MY-PC._shaprint-ipps._tcp.local")
        dos.writeShort(ptrTarget.size)
        dos.write(ptrTarget)

        // Additional 1: SRV
        writeDomainName(dos, "MY-PC._shaprint-ipps._tcp.local")
        dos.writeShort(DnsPacket.TYPE_SRV)
        dos.writeShort(DnsPacket.CLASS_IN)
        dos.writeInt(120) // TTL
        val srvTarget = encodeDomainName("MY-PC.local")
        dos.writeShort(6 + srvTarget.size)
        dos.writeShort(0)     // priority
        dos.writeShort(0)     // weight
        dos.writeShort(48631) // port
        dos.write(srvTarget)

        // Additional 2: TXT
        writeDomainName(dos, "MY-PC._shaprint-ipps._tcp.local")
        dos.writeShort(DnsPacket.TYPE_TXT)
        dos.writeShort(DnsPacket.CLASS_IN)
        dos.writeInt(120) // TTL

        val txtEntries = listOf("rp=ipp/print", "name=Office Printer Host", "queue=Epson-L3210", "queue=HP-LaserJet")
        val txtBytes = ByteArrayOutputStream()
        for (entry in txtEntries) {
            val eb = entry.toByteArray(Charsets.UTF_8)
            txtBytes.write(eb.size)
            txtBytes.write(eb)
        }
        val txtData = txtBytes.toByteArray()
        dos.writeShort(txtData.size)
        dos.write(txtData)

        val packetData = baos.toByteArray()
        val sender = InetAddress.getByName("192.168.1.55")

        val discovered = DnsPacket.parseResponse(packetData, packetData.size, sender)
        assertNotNull(discovered)
        assertEquals("192.168.1.55", discovered?.host)
        assertEquals(48631, discovered?.port)
        assertEquals("Office Printer Host", discovered?.computerName)
        assertEquals(listOf("Epson-L3210", "HP-LaserJet"), discovered?.advertisedQueues)
        assertFalse(discovered?.isGoodbye ?: true)
    }

    @Test
    fun detectsGoodbyePacketWithZeroTtl() {
        val baos = ByteArrayOutputStream()
        val dos = DataOutputStream(baos)

        dos.writeShort(0x0000)
        dos.writeShort(0x8400)
        dos.writeShort(0)
        dos.writeShort(1)
        dos.writeShort(0)
        dos.writeShort(0)

        writeDomainName(dos, "_shaprint-ipps._tcp.local")
        dos.writeShort(DnsPacket.TYPE_PTR)
        dos.writeShort(DnsPacket.CLASS_IN)
        dos.writeInt(0) // TTL = 0 -> goodbye
        val ptrTarget = encodeDomainName("SHUTDOWN-PC._shaprint-ipps._tcp.local")
        dos.writeShort(ptrTarget.size)
        dos.write(ptrTarget)

        val packetData = baos.toByteArray()
        val sender = InetAddress.getByName("192.168.1.99")

        val discovered = DnsPacket.parseResponse(packetData, packetData.size, sender)
        assertNotNull(discovered)
        assertTrue(discovered?.isGoodbye ?: false)
    }

    @Test
    fun rejectsMalformedOrTruncatedPackets() {
        val sender = InetAddress.getByName("127.0.0.1")
        assertNull(DnsPacket.parseResponse(ByteArray(4), 4, sender))
        assertNull(DnsPacket.parseResponse(ByteArray(0), 0, sender))
    }

    @Test
    fun ignoresNonShaPrintMdnsResponses() {
        val baos = ByteArrayOutputStream()
        val dos = DataOutputStream(baos)

        dos.writeShort(0x0000)
        dos.writeShort(0x8400)
        dos.writeShort(0)
        dos.writeShort(1)
        dos.writeShort(0)
        dos.writeShort(0)

        writeDomainName(dos, "_googlecast._tcp.local")
        dos.writeShort(DnsPacket.TYPE_PTR)
        dos.writeShort(DnsPacket.CLASS_IN)
        dos.writeInt(120)
        val ptrTarget = encodeDomainName("LivingRoom-TV._googlecast._tcp.local")
        dos.writeShort(ptrTarget.size)
        dos.write(ptrTarget)

        val packetData = baos.toByteArray()
        val sender = InetAddress.getByName("192.168.1.77")

        assertNull(DnsPacket.parseResponse(packetData, packetData.size, sender))
    }

    @Test
    fun rejectsOutOfBoundsCompressionPointerBeyondPacketLength() {
        val baos = ByteArrayOutputStream()
        val dos = DataOutputStream(baos)

        dos.writeShort(0x0000)
        dos.writeShort(0x8400)
        dos.writeShort(0)
        dos.writeShort(1)
        dos.writeShort(0)
        dos.writeShort(0)

        // Compression pointer pointing to offset 300 (0xC12C), which is < 4096 buffer size but > packet length
        dos.writeByte(0xC1)
        dos.writeByte(0x2C)

        val smallPacket = baos.toByteArray()
        val reusableBuffer = ByteArray(4096)
        System.arraycopy(smallPacket, 0, reusableBuffer, 0, smallPacket.size)

        val sender = InetAddress.getByName("192.168.1.88")
        // Should return null cleanly without throwing IllegalArgumentException
        assertNull(DnsPacket.parseResponse(reusableBuffer, smallPacket.size, sender))
    }

    private fun writeDomainName(dos: DataOutputStream, domain: String) {
        val bytes = encodeDomainName(domain)
        dos.write(bytes)
    }

    private fun encodeDomainName(domain: String): ByteArray {
        val baos = ByteArrayOutputStream()
        val labels = domain.trimEnd('.').split('.')
        for (label in labels) {
            val b = label.toByteArray(Charsets.UTF_8)
            baos.write(b.size)
            baos.write(b)
        }
        baos.write(0)
        return baos.toByteArray()
    }
}
