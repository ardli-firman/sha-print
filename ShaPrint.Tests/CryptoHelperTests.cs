using ShaPrint.Core;
using System;
using System.Security.Cryptography;
using System.Text;
using Xunit;

namespace ShaPrint.Tests;

public class CryptoHelperTests
{
    // ── AES-GCM ──────────────────────────────────────

    [Fact]
    public void EncryptAesGcm_RoundTrip_ProducesOriginalPlaintext()
    {
        byte[] original = Encoding.UTF8.GetBytes("Hello, this is a test print job payload!");
        byte[] encrypted = CryptoHelper.EncryptAesGcm(original);
        byte[] decrypted = CryptoHelper.DecryptAesGcm(encrypted);

        Assert.Equal(original, decrypted);
    }

    [Fact]
    public void EncryptAesGcm_DifferentPlaintexts_ProduceDifferentCiphertexts()
    {
        byte[] a = Encoding.UTF8.GetBytes("AAAA");
        byte[] b = Encoding.UTF8.GetBytes("BBBB");

        byte[] encA = CryptoHelper.EncryptAesGcm(a);
        byte[] encB = CryptoHelper.EncryptAesGcm(b);

        Assert.NotEqual(encA, encB);
    }

    [Fact]
    public void EncryptAesGcm_SamePlaintextTwice_ProducesDifferentCiphertexts()
    {
        byte[] plain = Encoding.UTF8.GetBytes("same data");
        byte[] enc1 = CryptoHelper.EncryptAesGcm(plain);
        byte[] enc2 = CryptoHelper.EncryptAesGcm(plain);

        // Nonces must be random → ciphertexts differ
        Assert.NotEqual(enc1, enc2);
    }

    [Fact]
    public void EncryptAesGcm_NonceAndTagArePresent()
    {
        byte[] plain = Encoding.UTF8.GetBytes("test");
        byte[] encrypted = CryptoHelper.EncryptAesGcm(plain);

        // 12 byte nonce + plaintext len + 16 byte tag
        Assert.True(encrypted.Length >= 12 + 16);
        Assert.Equal(plain.Length + 12 + 16, encrypted.Length);
    }

    [Fact]
    public void DecryptAesGcm_TamperedCiphertext_ThrowsCryptographicException()
    {
        byte[] plain = Encoding.UTF8.GetBytes("important document");
        byte[] encrypted = CryptoHelper.EncryptAesGcm(plain);

        // Flip a byte in the ciphertext region
        encrypted[13] ^= 0xFF;

        Assert.ThrowsAny<CryptographicException>(() => CryptoHelper.DecryptAesGcm(encrypted));
    }

    [Fact]
    public void DecryptAesGcm_TamperedTag_ThrowsCryptographicException()
    {
        byte[] plain = Encoding.UTF8.GetBytes("important document");
        byte[] encrypted = CryptoHelper.EncryptAesGcm(plain);

        // Flip the last byte (tag)
        encrypted[^1] ^= 0xFF;

        Assert.ThrowsAny<CryptographicException>(() => CryptoHelper.DecryptAesGcm(encrypted));
    }

    [Fact]
    public void DecryptAesGcm_EmptyData_Throws()
    {
        Assert.Throws<ArgumentException>(() => CryptoHelper.DecryptAesGcm(Array.Empty<byte>()));
    }

    [Fact]
    public void DecryptAesGcm_TooShortData_Throws()
    {
        // Need at least 12 (nonce) + 16 (tag) = 28 bytes
        Assert.Throws<ArgumentException>(() => CryptoHelper.DecryptAesGcm(new byte[20]));
    }

    [Fact]
    public void EncryptAesGcm_LargePayload_Works()
    {
        byte[] plain = new byte[1_000_000]; // 1 MB
        RandomNumberGenerator.Fill(plain);

        byte[] encrypted = CryptoHelper.EncryptAesGcm(plain);
        byte[] decrypted = CryptoHelper.DecryptAesGcm(encrypted);

        Assert.Equal(plain, decrypted);
    }

    [Fact]
    public void EncryptAesGcm_EmptyPayload_Works()
    {
        byte[] plain = Array.Empty<byte>();
        byte[] encrypted = CryptoHelper.EncryptAesGcm(plain);
        byte[] decrypted = CryptoHelper.DecryptAesGcm(encrypted);

        Assert.Empty(decrypted);
    }

    // ── HMAC ─────────────────────────────────────────

    [Fact]
    public void SignHmac_ProducesBase64String()
    {
        byte[] payload = Encoding.UTF8.GetBytes("test payload");
        string sig = CryptoHelper.SignHmac(payload);

        Assert.False(string.IsNullOrEmpty(sig));
        // Should be valid base64
        _ = Convert.FromBase64String(sig);
    }

    [Fact]
    public void VerifyHmac_ValidSignature_ReturnsTrue()
    {
        byte[] payload = Encoding.UTF8.GetBytes("discovery response data");
        string sig = CryptoHelper.SignHmac(payload);

        Assert.True(CryptoHelper.VerifyHmac(payload, sig));
    }

    [Fact]
    public void VerifyHmac_WrongSignature_ReturnsFalse()
    {
        byte[] payload = Encoding.UTF8.GetBytes("discovery response data");
        string realSig = CryptoHelper.SignHmac(payload);
        string fakeSig = CryptoHelper.SignHmac(Encoding.UTF8.GetBytes("different data"));

        Assert.False(CryptoHelper.VerifyHmac(payload, fakeSig));
    }

    [Fact]
    public void VerifyHmac_DifferentPayload_ReturnsFalse()
    {
        byte[] payload = Encoding.UTF8.GetBytes("original");
        string sig = CryptoHelper.SignHmac(payload);

        Assert.False(CryptoHelper.VerifyHmac(Encoding.UTF8.GetBytes("tampered"), sig));
    }

    // ── Config HMAC ──────────────────────────────────

    [Fact]
    public void WrapConfigWithHmac_RoundTrip_ReturnsOriginalJson()
    {
        string json = "{\"printers\":[\"Printer1\",\"Printer2\"]}";
        string wrapped = CryptoHelper.WrapConfigWithHmac(json);
        string? unwrapped = CryptoHelper.UnwrapConfigWithHmac(wrapped);

        Assert.NotNull(unwrapped);
        Assert.Equal(json, unwrapped);
    }

    [Fact]
    public void UnwrapConfigWithHmac_TamperedContent_ReturnsNull()
    {
        string json = "{\"printers\":[\"Printer1\"]}";
        string wrapped = CryptoHelper.WrapConfigWithHmac(json);

        // Tamper: change the json part
        string tampered = wrapped.Replace("Printer1", "EvilPrinter");

        string? result = CryptoHelper.UnwrapConfigWithHmac(tampered);
        Assert.Null(result);
    }

    [Fact]
    public void UnwrapConfigWithHmac_TamperedSignature_ReturnsNull()
    {
        string json = "{\"printers\":[\"Printer1\"]}";
        string wrapped = CryptoHelper.WrapConfigWithHmac(json);

        // Replace just the signature portion
        int sigStart = wrapped.LastIndexOf("<!--HMAC:") + "<!--HMAC:".Length;
        int sigLen = wrapped.Length - sigStart - 3; // minus "-->"
        string tampered = wrapped[..sigStart]
            + new string('A', sigLen) // dummy base64
            + "-->";

        string? result = CryptoHelper.UnwrapConfigWithHmac(tampered);
        Assert.Null(result);
    }

    [Fact]
    public void UnwrapConfigWithHmac_NoHmacMarker_ReturnsNull()
    {
        string plain = "{\"plain\":\"json without hmac\"}";
        Assert.Null(CryptoHelper.UnwrapConfigWithHmac(plain));
    }
}
