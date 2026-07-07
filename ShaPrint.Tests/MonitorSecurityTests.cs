using System;
using System.Security.Cryptography;
using System.Text;
using System.Text.Json;
using ShaPrint.Core;
using ShaPrint.Core.Network;
using Xunit;

namespace ShaPrint.Tests
{
    public class MonitorSecurityTests
    {
        [Fact]
        public void EncryptDecrypt_ServerStatusPayload_RoundTrip_Succeeds()
        {
            var originalPayload = new ServerStatusPayload
            {
                ServerName = "SRV-TEST",
                HostName = "SRV-TEST",
                NetworkChannel = "TestChannel",
                Version = "1.2.3.4",
                UptimeSeconds = 86400
            };

            string json = JsonSerializer.Serialize(originalPayload);
            byte[] rawBytes = Encoding.UTF8.GetBytes(json);

            // Encrypt using AES-256-GCM (CryptoHelper)
            byte[] encrypted = CryptoHelper.EncryptAesGcm(rawBytes);
            Assert.NotNull(encrypted);
            Assert.NotEqual(rawBytes, encrypted);

            // Decrypt
            byte[] decrypted = CryptoHelper.DecryptAesGcm(encrypted);
            string decryptedJson = Encoding.UTF8.GetString(decrypted);

            var decryptedPayload = JsonSerializer.Deserialize<ServerStatusPayload>(decryptedJson);
            Assert.NotNull(decryptedPayload);
            Assert.Equal(originalPayload.ServerName, decryptedPayload!.ServerName);
            Assert.Equal(originalPayload.NetworkChannel, decryptedPayload.NetworkChannel);
            Assert.Equal(originalPayload.Version, decryptedPayload.Version);
            Assert.Equal(originalPayload.UptimeSeconds, decryptedPayload.UptimeSeconds);
        }

        [Fact]
        public void Decrypt_TamperedCiphertext_ThrowsCryptographicException()
        {
            byte[] original = Encoding.UTF8.GetBytes("GET_STATUS");
            byte[] encrypted = CryptoHelper.EncryptAesGcm(original);

            // Tamper with one byte in the ciphertext
            encrypted[encrypted.Length - 1] ^= 0x01;

            Assert.ThrowsAny<CryptographicException>(() => CryptoHelper.DecryptAesGcm(encrypted));
        }
    }
}
