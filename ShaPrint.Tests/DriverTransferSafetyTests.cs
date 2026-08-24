using System;
using System.IO;
using System.Reflection;
using System.Security.Cryptography;
using System.Text;
using System.Text.Json;
using System.Threading;
using System.Threading.Tasks;
using ShaPrint.Core;
using ShaPrint.Core.Abstractions;
using ShaPrint.Core.Network;
using ShaPrint.WpfApp.Services.Client;
using Xunit;

namespace ShaPrint.Tests
{
    public sealed class DriverTransferSafetyTests
    {
        [Theory]
        [InlineData("")]
        [InlineData("short")]
        [InlineData("000000000000000000000000000000000000000000000000000000000000000g")]
        [InlineData("000000000000000000000000000000000000000000000000000000000000000 ")]
        public void PackageIdValidator_RejectsMalformedIds(string id)
        {
            Assert.False(DriverPackageIdValidator.IsValid(id));
        }

        [Fact]
        public void PackageIdValidator_AcceptsExactly64HexCharacters()
        {
            Assert.True(DriverPackageIdValidator.IsValid(new string('a', 64)));
            Assert.True(DriverPackageIdValidator.IsValid("0123456789abcdef0123456789ABCDEF0123456789abcdef0123456789ABCDEF"));
            Assert.False(DriverPackageIdValidator.IsValid(new string('a', 63)));
            Assert.False(DriverPackageIdValidator.IsValid(new string('a', 65)));
        }

        [Fact]
        public void DriverPackageRequestPacket_IsBoundedAndLittleEndian()
        {
            string packageId = new string('a', 64);
            byte[] packet = DriverPackageManager.BuildDriverPackageRequestPacket(new DriverPackageRequest
            {
                PrinterName = "Office Printer",
                DriverPackageId = packageId
            });

            int jsonLength = BitConverter.ToInt32(packet, sizeof(int));
            Assert.Equal(Constants.PacketTypeDriverPackageRequest, BitConverter.ToInt32(packet, 0));
            Assert.Equal(sizeof(int) * 2 + jsonLength, packet.Length);
            using var json = JsonDocument.Parse(packet.AsMemory(sizeof(int) * 2, jsonLength));
            Assert.Equal(packageId, json.RootElement.GetProperty("DriverPackageId").GetString());
            Assert.Throws<InvalidDataException>(() => DriverPackageManager.BuildDriverPackageRequestPacket(
                new DriverPackageRequest { PrinterName = new string('x', 20_000), DriverPackageId = packageId }));
            CryptographicOperations.ZeroMemory(packet);
        }

        [Fact]
        public void CancellationClassification_DistinguishesUserAndDeadline()
        {
            var user = DriverPackageManager.CreateCancellationResult(userCancelled: true);
            var deadline = DriverPackageManager.CreateCancellationResult(userCancelled: false);

            Assert.False(user.TimedOut);
            Assert.Contains("cancel", user.ErrorMessage!, StringComparison.OrdinalIgnoreCase);
            Assert.True(deadline.TimedOut);
            Assert.Contains("timed out", deadline.ErrorMessage!, StringComparison.OrdinalIgnoreCase);
        }

        [Fact]
        public void VerifiedCache_RejectsPartialDirectoryAndAcceptsExactMarker()
        {
            string root = Path.Combine(Path.GetTempPath(), "ShaPrint-driver-cache-" + Guid.NewGuid().ToString("N"));
            string packageId;
            byte[] package = Encoding.UTF8.GetBytes("verified package");
            try
            {
                packageId = Convert.ToHexString(SHA256.HashData(package)).ToLowerInvariant();
                Directory.CreateDirectory(root);
                File.WriteAllBytes(Path.Combine(root, "package.zip"), package);
                var marker = new DriverPackageVerifiedMarker
                {
                    Sha256 = packageId,
                    TotalSizeBytes = package.LongLength,
                    FileCount = 1,
                    ExtractedAtUtc = DateTime.UtcNow
                };
                File.WriteAllText(Path.Combine(root, ".verified.json"), JsonSerializer.Serialize(marker));

                Assert.True(DriverPackageManager.TryGetVerifiedCache(root, packageId, package.LongLength, out _));
                File.Delete(Path.Combine(root, ".verified.json"));
                Assert.False(DriverPackageManager.TryGetVerifiedCache(root, packageId, package.LongLength, out _));
            }
            finally
            {
                try { Directory.Delete(root, recursive: true); } catch { }
                CryptographicOperations.ZeroMemory(package);
            }
        }

        [Fact]
        public async Task ExistingPackageHashCheck_RejectsSameLengthCorruption()
        {
            string root = Path.Combine(Path.GetTempPath(), "ShaPrint-driver-final-" + Guid.NewGuid().ToString("N"));
            byte[] package = Encoding.UTF8.GetBytes("verified package");
            string packageId = Convert.ToHexString(SHA256.HashData(package)).ToLowerInvariant();
            try
            {
                Directory.CreateDirectory(root);
                File.WriteAllBytes(Path.Combine(root, "package.zip"), package);
                File.WriteAllText(Path.Combine(root, "manifest.json"), JsonSerializer.Serialize(new DriverPackageManifest
                {
                    Sha256 = packageId,
                    TotalSizeBytes = package.LongLength
                }));

                var service = new ShaPrint.Server.DriverPackageService(
                    new MockProcessRunner(), new ShaPrint.Core.Abstractions.RealFileSystem());
                var method = typeof(ShaPrint.Server.DriverPackageService).GetMethod(
                    "IsCompleteFinalPackageAsync", BindingFlags.Instance | BindingFlags.NonPublic);
                Assert.NotNull(method);
                var validTask = (Task<bool>)method!.Invoke(service, new object[] { root, packageId, CancellationToken.None })!;
                Assert.True(await validTask);

                package[0] ^= 0x01;
                File.WriteAllBytes(Path.Combine(root, "package.zip"), package);
                var corruptTask = (Task<bool>)method.Invoke(service, new object[] { root, packageId, CancellationToken.None })!;
                Assert.False(await corruptTask);
            }
            finally
            {
                try { Directory.Delete(root, recursive: true); } catch { }
                CryptographicOperations.ZeroMemory(package);
            }
        }

        [Fact]
        public async Task DriverPackageVerify_RejectsMalformedIdBeforeFileAccess()
        {
            Assert.False(await DriverPackageVerify.VerifyPackageAsync(
                Path.Combine(Path.GetTempPath(), "does-not-matter"), "short", 1));
            Assert.False(DriverPackageVerify.VerifyBytes(new byte[] { 1 }, "short", 1));
        }

        [Fact]
        public async Task DriverPackageService_CancellationStopsLocateProcess()
        {
            var service = new ShaPrint.Server.DriverPackageService(
                new CancellationProcessRunner(), new MockFileSystem());
            using var cancellation = new CancellationTokenSource(TimeSpan.FromMilliseconds(50));

            await Assert.ThrowsAnyAsync<OperationCanceledException>(() =>
                service.GetDriverPackageAsync("Test Driver", cancellation.Token));
        }

        private sealed class CancellationProcessRunner : IProcessRunner
        {
            public Task<ProcessResult> RunAsync(string fileName, string arguments, TimeSpan? timeout = null)
                => Task.FromResult(new ProcessResult { ExitCode = 1 });

            public async Task<ProcessResult> RunAsync(
                string fileName, string arguments, TimeSpan? timeout, CancellationToken cancellationToken)
            {
                await Task.Delay(Timeout.InfiniteTimeSpan, cancellationToken);
                return new ProcessResult { ExitCode = 1 };
            }
        }

        [Fact]
        public void ChunkSequence_RejectsDuplicateOutOfOrderAndGap()
        {
            var state = new DriverChunkSequence(3);
            Assert.True(state.TryAccept(0, 3, out _));
            Assert.False(state.TryAccept(0, 3, out _));
            Assert.False(state.TryAccept(2, 3, out _));
            Assert.True(state.TryAccept(1, 3, out _));
            Assert.True(state.TryAccept(2, 3, out _));
            Assert.True(state.IsComplete);
        }

        [Fact]
        public async Task RealProcessRunner_TimeoutReturnsFailureWithoutWaitingForOutput()
        {
            var runner = new RealProcessRunner();
            var started = DateTime.UtcNow;

            var result = await runner.RunAsync(
                OperatingSystem.IsWindows() ? "powershell.exe" : "sh",
                OperatingSystem.IsWindows() ? "-NoProfile -Command \"Start-Sleep -Seconds 30\"" : "-c \"sleep 30\"",
                TimeSpan.FromMilliseconds(200));

            Assert.False(result.Success);
            Assert.Contains("timed out", result.Error, StringComparison.OrdinalIgnoreCase);
            Assert.True(DateTime.UtcNow - started < TimeSpan.FromSeconds(5));
        }
    }
}
