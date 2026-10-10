package io.shaprint.client.core.network

import org.junit.Assert.*
import org.junit.Test
import java.security.MessageDigest

class TlsFingerprintTest {

    @Test
    fun computeSha256ProducesColonDelimitedUppercaseHex() {
        val sampleDer = "ShaPrint Self-Signed Certificate Test".toByteArray(Charsets.UTF_8)
        val fingerprint = TlsFingerprint.computeSha256(sampleDer)

        // 32 bytes -> 32 hex pairs with 31 colons = 95 characters
        assertEquals(95, fingerprint.length)
        assertTrue(fingerprint.contains(":"))
        assertEquals(fingerprint, fingerprint.uppercase())

        val manualDigest = MessageDigest.getInstance("SHA-256").digest(sampleDer)
        val expected = manualDigest.joinToString(":") { String.format("%02X", it) }
        assertEquals(expected, fingerprint)
    }

    @Test
    fun matchesIsCaseAndColonInsensitive() {
        val fp1 = "AA:BB:CC:DD:EE:FF"
        val fp2 = "aa:bb:cc:dd:ee:ff"
        val fp3 = "AABBCCDDEEFF"
        val fp4 = "11:22:33:44:55:66"

        assertTrue(TlsFingerprint.matches(fp1, fp2))
        assertTrue(TlsFingerprint.matches(fp1, fp3))
        assertFalse(TlsFingerprint.matches(fp1, fp4))
    }
}
