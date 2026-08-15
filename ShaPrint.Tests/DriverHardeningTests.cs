using System;
using System.IO;
using System.Text.Json;
using System.Threading;
using System.Threading.Tasks;
using ShaPrint.Core;
using ShaPrint.WpfApp.Services.Client;
using Xunit;

namespace ShaPrint.Tests
{
    /// <summary>
    /// SM1/SM4/SM4b/SM5a-c/SM6/SM8 — Unit tests for driver provisioning hardening (H1, H3, H4, H5, H7, H8).
    /// Tests timeout semantics, cache re-verification, deterministic .inf selection, and size-cap.
    /// </summary>
    public class DriverHardeningTests : IDisposable
    {
        private readonly string _tempDir;

        public DriverHardeningTests()
        {
            _tempDir = Path.Combine(Path.GetTempPath(), "ShaPrintHardeningTests_" + Guid.NewGuid().ToString("N"));
            Directory.CreateDirectory(_tempDir);
        }

        public void Dispose()
        {
            if (Directory.Exists(_tempDir))
                Directory.Delete(_tempDir, true);
        }

        // ═══════════════════════════════════════════════════════════════════
        // H1 (SM1): Per-read timeout semantics
        // ═══════════════════════════════════════════════════════════════════

        [Fact]
        public void Constants_DriverTransferReadTimeoutMs_Is30Seconds()
        {
            // Verify the per-read timeout constant is 30 seconds
            Assert.Equal(30_000, Constants.DriverTransferReadTimeoutMs);
        }

        [Fact]
        public void DriverDownloadResult_TimedOut_DefaultsFalse()
        {
            // The TimedOut flag should default to false
            var result = new DriverDownloadResult();
            Assert.False(result.TimedOut);
        }

        // ═══════════════════════════════════════════════════════════════════
        // H3 (SM4/SM4b): Cache re-verification via .verified.json
        // ═══════════════════════════════════════════════════════════════════

        [Fact]
        public void DriverPackageVerifiedMarker_DefaultValues_AreCorrect()
        {
            var marker = new DriverPackageVerifiedMarker();
            Assert.Equal(string.Empty, marker.Sha256);
            Assert.Equal(0, marker.TotalSizeBytes);
            Assert.Equal(0, marker.FileCount);
            Assert.Equal(default, marker.ExtractedAtUtc);
        }

        [Fact]
        public void DriverPackageVerifiedMarker_SerializesDeserializesCorrectly()
        {
            // Arrange
            var marker = new DriverPackageVerifiedMarker
            {
                Sha256 = "abcdef1234567890abcdef1234567890abcdef1234567890abcdef1234567890",
                TotalSizeBytes = 15728640,
                FileCount = 47,
                ExtractedAtUtc = new DateTime(2026, 8, 15, 14, 30, 0, DateTimeKind.Utc)
            };

            // Act
            string json = JsonSerializer.Serialize(marker);
            var deserialized = JsonSerializer.Deserialize<DriverPackageVerifiedMarker>(json);

            // Assert
            Assert.NotNull(deserialized);
            Assert.Equal(marker.Sha256, deserialized.Sha256);
            Assert.Equal(marker.TotalSizeBytes, deserialized.TotalSizeBytes);
            Assert.Equal(marker.FileCount, deserialized.FileCount);
        }

        // ═══════════════════════════════════════════════════════════════════
        // H4 (SM5a/SM5b/SM5c): Deterministic .inf selection (ResolveInfPath)
        // ═══════════════════════════════════════════════════════════════════

        [Fact]
        public void ResolveInfPath_ManifestInfName_ReturnsManifestEntry()
        {
            // Arrange: 2 .inf files, manifest.InfName matches one
            string pkgDir = Path.Combine(_tempDir, "pkg_manifest");
            Directory.CreateDirectory(pkgDir);
            File.WriteAllText(Path.Combine(pkgDir, "oem25.inf"), "[Version]");
            File.WriteAllText(Path.Combine(pkgDir, "oem26.inf"), "[Version]");

            var manager = new DriverPackageManager();

            // Act
            string? result = manager.ResolveInfPath(pkgDir, "oem26.inf");

            // Assert
            Assert.NotNull(result);
            Assert.Equal("oem26.inf", Path.GetFileName(result));
        }

        [Fact]
        public void ResolveInfPath_SoleInf_ReturnsIt()
        {
            // Arrange: 1 .inf file
            string pkgDir = Path.Combine(_tempDir, "pkg_sole");
            Directory.CreateDirectory(pkgDir);
            File.WriteAllText(Path.Combine(pkgDir, "myprinter.inf"), "[Version]");

            var manager = new DriverPackageManager();

            // Act
            string? result = manager.ResolveInfPath(pkgDir);

            // Assert
            Assert.NotNull(result);
            Assert.Equal("myprinter.inf", Path.GetFileName(result));
        }

        [Fact]
        public void ResolveInfPath_MultipleOemInfsNoManifest_PicksFirstOem()
        {
            // Arrange: 2 oem*.inf files, no manifest
            // New behavior: Priority 3 picks first oem*.inf (best-effort) rather than failing,
            // because ambiguity between oem INFs is a known pnputil export pattern.
            string pkgDir = Path.Combine(_tempDir, "pkg_ambiguous");
            Directory.CreateDirectory(pkgDir);
            File.WriteAllText(Path.Combine(pkgDir, "oem25.inf"), "[Version]");
            File.WriteAllText(Path.Combine(pkgDir, "oem26.inf"), "[Version]");

            var manager = new DriverPackageManager();

            // Act
            string? result = manager.ResolveInfPath(pkgDir);

            // Assert — picks an oem* INF (best-effort), not null
            Assert.NotNull(result);
            Assert.StartsWith("oem", Path.GetFileName(result), StringComparison.OrdinalIgnoreCase);
        }

        [Fact]
        public void ResolveInfPath_ManifestNotFound_FallsToSoleInf()
        {
            // Arrange: 1 .inf, manifest.InfName doesn't match
            string pkgDir = Path.Combine(_tempDir, "pkg_nomatch");
            Directory.CreateDirectory(pkgDir);
            File.WriteAllText(Path.Combine(pkgDir, "oem25.inf"), "[Version]");

            var manager = new DriverPackageManager();

            // Act — manifest says oem99.inf but only oem25.inf exists
            string? result = manager.ResolveInfPath(pkgDir, "oem99.inf");

            // Assert — falls through to sole .inf
            Assert.NotNull(result);
            Assert.Equal("oem25.inf", Path.GetFileName(result));
        }

        [Fact]
        public void ResolveInfPath_NoInfFiles_ReturnsNull()
        {
            // Arrange: empty directory
            string pkgDir = Path.Combine(_tempDir, "pkg_empty");
            Directory.CreateDirectory(pkgDir);

            var manager = new DriverPackageManager();

            // Act
            string? result = manager.ResolveInfPath(pkgDir);

            // Assert
            Assert.Null(result);
        }

        [Fact]
        public void ResolveInfPath_MultipleInfWithManifest_MatchesCorrectly()
        {
            // Arrange: 3 .inf files, manifest picks one (Priority 1 exact match)
            string pkgDir = Path.Combine(_tempDir, "pkg_multi_manifest");
            Directory.CreateDirectory(pkgDir);
            File.WriteAllText(Path.Combine(pkgDir, "aaa.inf"), "[Version]");
            File.WriteAllText(Path.Combine(pkgDir, "bbb.inf"), "[Version]");
            File.WriteAllText(Path.Combine(pkgDir, "ccc.inf"), "[Version]");

            var manager = new DriverPackageManager();

            // Act
            string? result = manager.ResolveInfPath(pkgDir, "BBB.inf"); // case-insensitive

            // Assert — manifest exact match wins regardless of how many INFs exist
            Assert.NotNull(result);
            Assert.Equal("bbb.inf", Path.GetFileName(result));
        }

        [Fact]
        public void ResolveInfPath_SystemInfExcluded_ReturnsRealDriverInf()
        {
            // Arrange: ntprint.inf (system) + real driver INF, no manifest
            // pnputil commonly co-exports ntprint.inf as a dependency
            string pkgDir = Path.Combine(_tempDir, "pkg_system_inf");
            Directory.CreateDirectory(pkgDir);
            File.WriteAllText(Path.Combine(pkgDir, "ntprint.inf"), "[Version]");
            File.WriteAllText(Path.Combine(pkgDir, "EPSONL3210.inf"), "[Version]");

            var manager = new DriverPackageManager();

            // Act — no manifest provided
            string? result = manager.ResolveInfPath(pkgDir);

            // Assert — ntprint.inf filtered out; EPSONL3210.inf returned
            Assert.NotNull(result);
            Assert.Equal("EPSONL3210.inf", Path.GetFileName(result), ignoreCase: true);
        }

        // ═══════════════════════════════════════════════════════════════════
        // H5 (SM6): Size-cap enforcement
        // ═══════════════════════════════════════════════════════════════════

        [Fact]
        public void Constants_MaxDriverPackageSize_Is200MB()
        {
            Assert.Equal(209_715_200, Constants.MaxDriverPackageSize);
        }

        [Fact]
        public async Task Download_OversizePackage_ReturnsErrorBeforeConnect()
        {
            // Arrange: expectedSize > MaxDriverPackageSize (200 MB)
            var manager = new DriverPackageManager();

            // Act
            var result = await manager.DownloadDriverPackageAsync(
                "127.0.0.1", "TestPrinter", new string('a', 64), 300_000_000);

            // Assert — should fail without even attempting TCP connect
            Assert.False(result.Success);
            Assert.NotNull(result.ErrorMessage);
            Assert.Contains("exceeds limit", result.ErrorMessage, StringComparison.OrdinalIgnoreCase);
        }

        [Fact]
        public async Task Download_ZeroSizePackage_ReturnsError()
        {
            // Arrange: expectedSize = 0
            var manager = new DriverPackageManager();

            // Act
            var result = await manager.DownloadDriverPackageAsync(
                "127.0.0.1", "TestPrinter", new string('a', 64), 0);

            // Assert
            Assert.False(result.Success);
            Assert.NotNull(result.ErrorMessage);
            Assert.Contains("exceeds limit", result.ErrorMessage, StringComparison.OrdinalIgnoreCase);
        }

        [Fact]
        public async Task Download_NegativeSizePackage_ReturnsError()
        {
            // Arrange: expectedSize < 0
            var manager = new DriverPackageManager();

            // Act
            var result = await manager.DownloadDriverPackageAsync(
                "127.0.0.1", "TestPrinter", new string('a', 64), -100);

            // Assert
            Assert.False(result.Success);
            Assert.NotNull(result.ErrorMessage);
        }

        // ═══════════════════════════════════════════════════════════════════
        // H7: Per-chunk sanity cap constant
        // ═══════════════════════════════════════════════════════════════════

        [Fact]
        public void Constants_DriverPackageChunkSize_Is64KB()
        {
            Assert.Equal(65_536, Constants.DriverPackageChunkSize);
        }

        // ═══════════════════════════════════════════════════════════════════
        // H2 constants (D1)
        // ═══════════════════════════════════════════════════════════════════

        [Fact]
        public void Constants_DriverExtractMaxEntryCount_Is1024()
        {
            Assert.Equal(1024, Constants.DriverExtractMaxEntryCount);
        }

        [Fact]
        public void Constants_DriverExtractMaxTotalBytes_Is1GiB()
        {
            Assert.Equal(1_073_741_824, Constants.DriverExtractMaxTotalBytes);
        }

        [Fact]
        public void Constants_DriverExtractMaxEntryNameLength_Is512()
        {
            Assert.Equal(512, Constants.DriverExtractMaxEntryNameLength);
        }
    }
}
