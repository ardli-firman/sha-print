using System;
using System.Threading;
using System.Threading.Tasks;
using ShaPrint.Core;
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
