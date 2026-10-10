package io.shaprint.client.core.security

import org.junit.Assert.*
import org.junit.Test

class KeystoreCryptoTest {

    @Test
    fun encryptAndDecryptRoundTripRestoresOriginalText() {
        val crypto = KeystoreCrypto(allowInMemoryFallbackForTest = true)
        val plain = "my-secret-network-channel-1234"

        val encrypted = crypto.encrypt(plain)
        assertNotEquals(plain, encrypted)
        assertTrue(encrypted.isNotEmpty())

        val decrypted = crypto.decrypt(encrypted)
        assertEquals(plain, decrypted)
    }

    @Test
    fun decryptCorruptedPayloadReturnsNullSafely() {
        val crypto = KeystoreCrypto(allowInMemoryFallbackForTest = true)
        assertNull(crypto.decrypt("invalid-base64-not-valid"))
        assertNull(crypto.decrypt("AQIDBA==")) // Too short to contain GCM IV
    }

    @Test(expected = SecurityException::class)
    fun throwsSecurityExceptionInProductionModeWhenAndroidKeyStoreUnavailable() {
        val productionCrypto = KeystoreCrypto(allowInMemoryFallbackForTest = false)
        productionCrypto.encrypt("should-not-fallback-silently")
    }
}
