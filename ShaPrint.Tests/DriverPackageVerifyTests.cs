using System;
using System.IO;
using System.Security.Cryptography;
using System.Text;
using System.Threading.Tasks;
using ShaPrint.Platform.Windows;
using Xunit;

namespace ShaPrint.Tests
{
    /// <summary>
    /// T16 — Unit tests for DriverPackageVerify (client-side integrity).
    /// NON-NEGOTIABLE: verifies SHA-256 + size check against HMAC-authenticated metadata.
    /// </summary>
    public class DriverPackageVerifyTests : IDisposable
    {
        private readonly string _tempDir;

        public DriverPackageVerifyTests()
        {
            _tempDir = Path.Combine(Path.GetTempPath(), "ShaPrintVerifyTests_" + Guid.NewGuid().ToString("N"));
            Directory.CreateDirectory(_tempDir);
        }

        public void Dispose()
        {
            if (Directory.Exists(_tempDir))
                Directory.Delete(_tempDir, true);
        }

        // ── T12: Valid package → returns true ──────────────────────────────

        [Fact]
        public async Task VerifyDriverPackage_ValidPackage_ReturnsTrue()
        {
            // Arrange
            byte[] content = Encoding.UTF8.GetBytes("Valid driver package content here with enough bytes");
            string expectedHash = Convert.ToHexString(SHA256.HashData(content)).ToLowerInvariant();
            long expectedSize = content.Length;

            string filePath = Path.Combine(_tempDir, "package.bin");
            await File.WriteAllBytesAsync(filePath, content);

            // Act
            bool result = await DriverPackageVerify.VerifyPackageAsync(filePath, expectedHash, expectedSize);

            // Assert
            Assert.True(result);
        }

        // ── T13: Size mismatch → returns false ─────────────────────────────

        [Fact]
        public async Task VerifyDriverPackage_SizeMismatch_ReturnsFalse()
        {
            // Arrange
            byte[] content = Encoding.UTF8.GetBytes("Some driver content");
            string expectedHash = Convert.ToHexString(SHA256.HashData(content)).ToLowerInvariant();
            long wrongSize = content.Length + 100; // wrong size

            string filePath = Path.Combine(_tempDir, "package.bin");
            await File.WriteAllBytesAsync(filePath, content);

            // Act
            bool result = await DriverPackageVerify.VerifyPackageAsync(filePath, expectedHash, wrongSize);

            // Assert
            Assert.False(result);
        }

        // ── T14: Hash mismatch → returns false ─────────────────────────────

        [Fact]
        public async Task VerifyDriverPackage_HashMismatch_ReturnsFalse()
        {
            // Arrange
            byte[] content = Encoding.UTF8.GetBytes("Some driver content");
            string wrongHash = new string('0', 64); // wrong hash
            long expectedSize = content.Length;

            string filePath = Path.Combine(_tempDir, "package.bin");
            await File.WriteAllBytesAsync(filePath, content);

            // Act
            bool result = await DriverPackageVerify.VerifyPackageAsync(filePath, wrongHash, expectedSize);

            // Assert
            Assert.False(result);
        }

        // ── T15: File corrupted (shorter) → returns false ──────────────────

        [Fact]
        public async Task VerifyDriverPackage_FileCorrupted_ReturnsFalse()
        {
            // Arrange
            byte[] originalContent = Encoding.UTF8.GetBytes("Original full driver package data that is quite long");
            string originalHash = Convert.ToHexString(SHA256.HashData(originalContent)).ToLowerInvariant();
            long originalSize = originalContent.Length;

            // Write truncated file
            byte[] truncated = new byte[originalContent.Length / 2];
            Array.Copy(originalContent, truncated, truncated.Length);
            string filePath = Path.Combine(_tempDir, "corrupted.bin");
            await File.WriteAllBytesAsync(filePath, truncated);

            // Act — size check will fail (shorter file)
            bool result = await DriverPackageVerify.VerifyPackageAsync(filePath, originalHash, originalSize);

            // Assert
            Assert.False(result);
        }

        // ── T16: Non-existent file → returns false ─────────────────────────

        [Fact]
        public async Task VerifyDriverPackage_NonExistentFile_ReturnsFalse()
        {
            // Arrange
            string nonExistentPath = Path.Combine(_tempDir, "does_not_exist.bin");

            // Act
            bool result = await DriverPackageVerify.VerifyPackageAsync(nonExistentPath, new string('a', 64), 100);

            // Assert
            Assert.False(result);
        }

        // ── Bonus: VerifyBytes in-memory path ──────────────────────────────

        [Fact]
        public void VerifyBytes_ValidData_ReturnsTrue()
        {
            // Arrange
            byte[] data = Encoding.UTF8.GetBytes("In-memory driver package bytes for testing");
            string expectedHash = Convert.ToHexString(SHA256.HashData(data)).ToLowerInvariant();

            // Act
            bool result = DriverPackageVerify.VerifyBytes(data, expectedHash, data.Length);

            // Assert
            Assert.True(result);
        }

        [Fact]
        public void VerifyBytes_EmptyData_ReturnsFalse()
        {
            // Act
            bool result = DriverPackageVerify.VerifyBytes(Array.Empty<byte>(), new string('a', 64), 0);

            // Assert
            Assert.False(result);
        }
    }
}
