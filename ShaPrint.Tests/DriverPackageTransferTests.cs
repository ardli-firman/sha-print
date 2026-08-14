using System;
using System.IO;
using System.Security.Cryptography;
using System.Text;
using System.Text.Json;
using System.Threading;
using System.Threading.Tasks;
using ShaPrint.Core.Network;
using Xunit;

namespace ShaPrint.Tests
{
    /// <summary>
    /// T15 — Unit tests for DriverPackage transfer protocol messages.
    /// Tests the 4 packet types: Request, Chunk, Complete, Error (plan Q1).
    /// </summary>
    public class DriverPackageTransferTests
    {
        // ── T7: DriverPackageRequest serialization ─────────────────────────

        [Fact]
        public void RequestDriverPackage_ValidRequest_SerializesCorrectly()
        {
            // Arrange
            var request = new DriverPackageRequest
            {
                PrinterName = "EPSON L120",
                DriverPackageId = new string('a', 64)
            };

            // Act
            string json = JsonSerializer.Serialize(request);
            var deserialized = JsonSerializer.Deserialize<DriverPackageRequest>(json);

            // Assert
            Assert.NotNull(deserialized);
            Assert.Equal("EPSON L120", deserialized.PrinterName);
            Assert.Equal(new string('a', 64), deserialized.DriverPackageId);
        }

        // ── T8: DriverPackageChunk — chunk data and HMAC preserved ─────────

        [Fact]
        public void ChunkTransfer_ChunkDataPreserved_PerChunk()
        {
            // Arrange
            byte[] chunkData = new byte[64 * 1024]; // 64 KB
            RandomNumberGenerator.Fill(chunkData);
            string hmac = Convert.ToHexString(SHA256.HashData(chunkData)).ToLowerInvariant();

            var chunk = new DriverPackageChunk
            {
                ChunkIndex = 0,
                TotalChunks = 3,
                Data = chunkData,
                ChunkHmac = hmac
            };

            // Act
            string json = JsonSerializer.Serialize(chunk);
            var deserialized = JsonSerializer.Deserialize<DriverPackageChunk>(json);

            // Assert
            Assert.NotNull(deserialized);
            Assert.Equal(0, deserialized.ChunkIndex);
            Assert.Equal(3, deserialized.TotalChunks);
            Assert.Equal(chunkData.Length, deserialized.Data.Length);
            Assert.Equal(hmac, deserialized.ChunkHmac);
        }

        // ── T9: DriverPackageError serializes correctly ────────────────────

        [Fact]
        public void RequestDriverPackage_ServerReturnsError_DeserializesCorrectly()
        {
            // Arrange
            var error = new DriverPackageError
            {
                Message = "Driver package not found on server."
            };

            // Act
            string json = JsonSerializer.Serialize(error);
            var deserialized = JsonSerializer.Deserialize<DriverPackageError>(json);

            // Assert
            Assert.NotNull(deserialized);
            Assert.Equal("Driver package not found on server.", deserialized.Message);
        }

        // ── T10: Per-chunk HMAC integrity verification ─────────────────────

        [Fact]
        public void ChunkTransfer_IntegrityCheck_PerChunk()
        {
            // Arrange
            byte[] rawData = Encoding.UTF8.GetBytes("This is test driver data for chunk 0");
            string expectedHmac = Convert.ToHexString(SHA256.HashData(rawData)).ToLowerInvariant();

            var chunk = new DriverPackageChunk
            {
                ChunkIndex = 0,
                TotalChunks = 1,
                Data = rawData,
                ChunkHmac = expectedHmac
            };

            // Act — compute HMAC of received data
            byte[] receivedHash = SHA256.HashData(chunk.Data);
            string actualHmac = Convert.ToHexString(receivedHash).ToLowerInvariant();

            // Assert: HMAC matches
            Assert.Equal(expectedHmac, actualHmac);

            // Tamper with data and verify mismatch
            chunk.Data[0] ^= 0xFF;
            byte[] tamperedHash = SHA256.HashData(chunk.Data);
            string tamperedHmac = Convert.ToHexString(tamperedHash).ToLowerInvariant();
            Assert.NotEqual(expectedHmac, tamperedHmac);
        }

        // ── T11: DriverPackageComplete — transfer metadata ─────────────────

        [Fact]
        public void TransferProgress_CompleteMetadata_Preserved()
        {
            // Arrange
            long totalBytes = 15_728_640;
            byte[] manifestJson = JsonSerializer.SerializeToUtf8Bytes(new DriverPackageManifest
            {
                InfName = "oem25.inf",
                DriverName = "EPSON L120",
                Sha256 = new string('b', 64),
                TotalSizeBytes = totalBytes,
                FileCount = 4
            });
            string manifestHmac = Convert.ToHexString(SHA256.HashData(manifestJson)).ToLowerInvariant();

            var complete = new DriverPackageComplete
            {
                TotalBytes = totalBytes,
                TotalChunks = 240,
                ManifestHmac = manifestHmac
            };

            // Act
            string json = JsonSerializer.Serialize(complete);
            var deserialized = JsonSerializer.Deserialize<DriverPackageComplete>(json);

            // Assert
            Assert.NotNull(deserialized);
            Assert.Equal(totalBytes, deserialized.TotalBytes);
            Assert.Equal(240, deserialized.TotalChunks);
            Assert.Equal(manifestHmac, deserialized.ManifestHmac);
        }
    }
}
