using System;
using System.IO;
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
