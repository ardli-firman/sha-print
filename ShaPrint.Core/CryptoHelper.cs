using System;
using System.Security.Cryptography;
using System.Text;

namespace ShaPrint.Core
{
    /// <summary>
    /// Provides AES-256-GCM encryption for TCP payloads and HMAC-SHA256
    /// signing for UDP discovery responses. All keys are derived from
    /// <see cref="Constants.SharedSecret"/>.
    /// </summary>
    public static class CryptoHelper
    {
        private const int AesKeySize = 32;     // 256 bits
        private const int AesNonceSize = 12;   // 96 bits (GCM recommended)
        private const int AesTagSize = 16;     // 128 bits
        private const int HmacKeySize = 32;

        private static readonly byte[] AesSalt = Encoding.UTF8.GetBytes("ShaPrint-AES-v1");
        private static readonly byte[] HmacSalt = Encoding.UTF8.GetBytes("ShaPrint-HMAC-v1");

        // ─────────────────────────────────────────────
        // Key derivation
        // ─────────────────────────────────────────────

        private static byte[] GetAesKey()
        {
            using var derive = new Rfc2898DeriveBytes(Constants.SharedSecret, AesSalt, 100_000, HashAlgorithmName.SHA256);
            return derive.GetBytes(AesKeySize);
        }

        private static byte[] GetHmacKey()
        {
            using var derive = new Rfc2898DeriveBytes(Constants.SharedSecret, HmacSalt, 100_000, HashAlgorithmName.SHA256);
            return derive.GetBytes(HmacKeySize);
        }

        // ─────────────────────────────────────────────
        // AES-256-GCM (for TCP print job payloads)
        // ─────────────────────────────────────────────

        /// <summary>
        /// Encrypts plaintext and returns [nonce (12) || ciphertext || tag (16)].
        /// </summary>
        public static byte[] EncryptAesGcm(byte[] plaintext)
        {
            byte[] key = GetAesKey();
            byte[] nonce = new byte[AesNonceSize];
            RandomNumberGenerator.Fill(nonce);

            byte[] ciphertext = new byte[plaintext.Length];
            byte[] tag = new byte[AesTagSize];

            using var aes = new AesGcm(key, AesTagSize);
            aes.Encrypt(nonce, plaintext, ciphertext, tag);

            // Concatenate: nonce + ciphertext + tag
            byte[] result = new byte[AesNonceSize + ciphertext.Length + AesTagSize];
            Buffer.BlockCopy(nonce, 0, result, 0, AesNonceSize);
            Buffer.BlockCopy(ciphertext, 0, result, AesNonceSize, ciphertext.Length);
            Buffer.BlockCopy(tag, 0, result, AesNonceSize + ciphertext.Length, AesTagSize);
            return result;
        }

        /// <summary>
        /// Decrypts a blob produced by <see cref="EncryptAesGcm"/>.
        /// Throws <see cref="CryptographicException"/> on tag mismatch.
        /// </summary>
        public static byte[] DecryptAesGcm(byte[] encrypted)
        {
            if (encrypted.Length < AesNonceSize + AesTagSize)
                throw new ArgumentException("Encrypted data too short.");

            byte[] key = GetAesKey();

            byte[] nonce = new byte[AesNonceSize];
            int ciphertextLen = encrypted.Length - AesNonceSize - AesTagSize;
            byte[] ciphertext = new byte[ciphertextLen];
            byte[] tag = new byte[AesTagSize];

            Buffer.BlockCopy(encrypted, 0, nonce, 0, AesNonceSize);
            Buffer.BlockCopy(encrypted, AesNonceSize, ciphertext, 0, ciphertextLen);
            Buffer.BlockCopy(encrypted, AesNonceSize + ciphertextLen, tag, 0, AesTagSize);

            byte[] plaintext = new byte[ciphertextLen];
            using var aes = new AesGcm(key, AesTagSize);
            aes.Decrypt(nonce, ciphertext, tag, plaintext);
            return plaintext;
        }

        // ─────────────────────────────────────────────
        // HMAC-SHA256 (for discovery response signing)
        // ─────────────────────────────────────────────

        /// <summary>
        /// Computes HMAC-SHA256 of the payload and returns it as base64.
        /// </summary>
        public static string SignHmac(byte[] payload)
        {
            byte[] key = GetHmacKey();
            using var hmac = new HMACSHA256(key);
            byte[] hash = hmac.ComputeHash(payload);
            return Convert.ToBase64String(hash);
        }

        /// <summary>
        /// Verifies an HMAC-SHA256 signature (base64) against a payload.
        /// Returns true if valid.
        /// </summary>
        public static bool VerifyHmac(byte[] payload, string expectedSignature)
        {
            string actual = SignHmac(payload);
            return CryptographicOperations.FixedTimeEquals(
                Encoding.UTF8.GetBytes(actual),
                Encoding.UTF8.GetBytes(expectedSignature));
        }

        // ─────────────────────────────────────────────
        // Config file integrity
        // ─────────────────────────────────────────────

        /// <summary>
        /// Wraps JSON content for storage with an HMAC integrity check.
        /// Returns: "{json}\n<!--HMAC:{base64_signature}-->"
        /// </summary>
        public static string WrapConfigWithHmac(string jsonContent)
        {
            string sig = SignHmac(Encoding.UTF8.GetBytes(jsonContent));
            return jsonContent + "\n<!--HMAC:" + sig + "-->";
        }

        /// <summary>
        /// Extracts and verifies the JSON content from an HMAC-wrapped config.
        /// Returns the JSON string on success, or null on tamper detection.
        /// </summary>
        public static string? UnwrapConfigWithHmac(string wrappedContent)
        {
            int markerIdx = wrappedContent.LastIndexOf("\n<!--HMAC:");
            if (markerIdx < 0)
                return null; // No HMAC present — legacy or tampered

            int sigStart = markerIdx + "\n<!--HMAC:".Length;
            int sigEnd = wrappedContent.IndexOf("-->", sigStart);
            if (sigEnd < 0)
                return null;

            string json = wrappedContent[..markerIdx];
            string expectedSig = wrappedContent[sigStart..sigEnd];

            if (VerifyHmac(Encoding.UTF8.GetBytes(json), expectedSig))
                return json;

            return null; // Signature mismatch
        }
    }
}
