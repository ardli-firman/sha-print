using ShaPrint.Server;
using System;
using System.Threading.Tasks;
using Xunit;

namespace ShaPrint.Tests
{
    public class SpoolerApiTests
    {
        [Fact]
        public async Task PrintRawDataAsync_InvalidPrinter_ReturnsFalseFast()
        {
            // Arrange
            string invalidPrinter = "NonExistentPrinter_999";
            byte[] data = new byte[] { 0x01, 0x02, 0x03 };
            string docName = "Test Doc";
            
            // We set a very long timeout, but it should return false quickly because OpenPrinter will fail
            var timeout = TimeSpan.FromSeconds(10);

            // Act
            var watch = System.Diagnostics.Stopwatch.StartNew();
            bool result = await SpoolerApi.PrintRawDataAsync(invalidPrinter, data, docName, timeout);
            watch.Stop();

            // Assert
            Assert.False(result, "PrintRawDataAsync should return false for an invalid printer.");
            Assert.True(watch.Elapsed < timeout, "The method should fail fast without waiting for the timeout if OpenPrinter fails.");
        }
    }
}
