using ShaPrint.Server;
using System;
using System.Collections.Generic;
using System.Runtime.InteropServices;
using System.Threading;
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

        [Fact]
        public async Task PrintRawDataAsync_PartialWrite_ReturnsFalseAndCleansUp()
        {
            var native = new FakeSpoolerNative { BytesWritten = 2 };

            bool result = await SpoolerApi.PrintRawDataAsync(
                "TestPrinter", new byte[] { 1, 2, 3 }, "Test", TimeSpan.FromSeconds(1), CancellationToken.None, native);

            Assert.False(result);
            Assert.Equal(new[] { "open", "doc", "page", "write", "end-page", "end-doc", "close" }, native.Calls);
        }

        [Fact]
        public async Task PrintRawDataAsync_StartPageFailure_EndsDocumentAndClosesPrinter()
        {
            var native = new FakeSpoolerNative { StartPageResult = false };

            bool result = await SpoolerApi.PrintRawDataAsync(
                "TestPrinter", new byte[] { 1 }, "Test", TimeSpan.FromSeconds(1), CancellationToken.None, native);

            Assert.False(result);
            Assert.Equal(new[] { "open", "doc", "page", "end-doc", "close" }, native.Calls);
        }

        [Fact]
        public async Task PrintRawDataAsync_WriteFailure_EndsPageAndDocumentAndClosesPrinter()
        {
            var native = new FakeSpoolerNative { WriteResult = false, BytesWritten = 0 };

            bool result = await SpoolerApi.PrintRawDataAsync(
                "TestPrinter", new byte[] { 1 }, "Test", TimeSpan.FromSeconds(1), CancellationToken.None, native);

            Assert.False(result);
            Assert.Equal(new[] { "open", "doc", "page", "write", "end-page", "end-doc", "close" }, native.Calls);
        }

        [Fact]
        public async Task PrintRawDataAsync_CleanupFailure_DoesNotReportSuccess()
        {
            var native = new FakeSpoolerNative { EndPageResult = false };

            bool result = await SpoolerApi.PrintRawDataAsync(
                "TestPrinter", new byte[] { 1 }, "Test", TimeSpan.FromSeconds(1), CancellationToken.None, native);

            Assert.False(result);
            Assert.Equal(new[] { "open", "doc", "page", "write", "end-page", "end-doc", "close" }, native.Calls);
        }

        [Fact]
        public async Task PrintRawDataAsync_EndDocFailure_DoesNotReportSuccess()
        {
            var native = new FakeSpoolerNative { EndDocResult = false };

            bool result = await SpoolerApi.PrintRawDataAsync(
                "TestPrinter", new byte[] { 1 }, "Test", TimeSpan.FromSeconds(1), CancellationToken.None, native);

            Assert.False(result);
            Assert.Equal(new[] { "open", "doc", "page", "write", "end-page", "end-doc", "close" }, native.Calls);
        }

        [Fact]
        public async Task PrintRawDataAsync_OpenFailure_DoesNotCloseInvalidHandle()
        {
            var native = new FakeSpoolerNative { OpenResult = false };

            bool result = await SpoolerApi.PrintRawDataAsync(
                "TestPrinter", new byte[] { 1 }, "Test", TimeSpan.FromSeconds(1), CancellationToken.None, native);

            Assert.False(result);
            Assert.Equal(new[] { "open" }, native.Calls);
        }

        [Fact]
        public async Task PrintRawDataAsync_Timeout_ReturnsPromptlyAndDefersCleanupUntilNativeOperationCompletes()
        {
            using var entered = new ManualResetEventSlim();
            using var release = new ManualResetEventSlim();
            var native = new FakeSpoolerNative
            {
                StartDocEntered = entered,
                StartDocRelease = release,
                StartDocResult = 42
            };

            var operation = SpoolerApi.PrintRawDataAsync(
                "TestPrinter", new byte[] { 1 }, "Test", TimeSpan.FromMilliseconds(50), CancellationToken.None, native);
            Assert.True(entered.Wait(TimeSpan.FromSeconds(1)));

            bool result = await operation;

            Assert.False(result);
            Assert.DoesNotContain("close", native.Calls);
            release.Set();
            Assert.True(SpinWait.SpinUntil(() => native.Calls.Contains("close"), TimeSpan.FromSeconds(1)));
        }

        [Fact]
        public async Task PrintRawDataAsync_Cancellation_ReturnsFalseWithoutRetry()
        {
            using var cancellation = new CancellationTokenSource();
            cancellation.Cancel();
            var native = new FakeSpoolerNative();

            bool result = await SpoolerApi.PrintRawDataAsync(
                "TestPrinter", new byte[] { 1 }, "Test", TimeSpan.FromSeconds(1), cancellation.Token, native);

            Assert.False(result);
            Assert.Empty(native.Calls);
        }

        private sealed class FakeSpoolerNative : ISpoolerApiNative
        {
            public List<string> Calls { get; } = new();
            public bool OpenResult { get; init; } = true;
            public int StartDocResult { get; init; } = 42;
            public bool StartPageResult { get; init; } = true;
            public bool WriteResult { get; init; } = true;
            public int BytesWritten { get; init; } = 1;
            public bool EndPageResult { get; init; } = true;
            public bool EndDocResult { get; init; } = true;
            public ManualResetEventSlim? StartDocEntered { get; init; }
            public ManualResetEventSlim? StartDocRelease { get; init; }

            public bool OpenPrinter(string printerName, out IntPtr handle)
            {
                Calls.Add("open");
                handle = new IntPtr(1);
                return OpenResult;
            }

            public bool ClosePrinter(IntPtr handle)
            {
                Calls.Add("close");
                return true;
            }

            public int StartDocPrinter(IntPtr handle, string documentName)
            {
                Calls.Add("doc");
                StartDocEntered?.Set();
                StartDocRelease?.Wait();
                return StartDocResult;
            }

            public bool EndDocPrinter(IntPtr handle)
            {
                Calls.Add("end-doc");
                return EndDocResult;
            }

            public bool StartPagePrinter(IntPtr handle)
            {
                Calls.Add("page");
                return StartPageResult;
            }

            public bool EndPagePrinter(IntPtr handle)
            {
                Calls.Add("end-page");
                return EndPageResult;
            }

            public bool WritePrinter(IntPtr handle, IntPtr bytes, int count, out int written)
            {
                Calls.Add("write");
                written = BytesWritten;
                return WriteResult;
            }

            public bool SetJob(IntPtr handle, int jobId)
            {
                Calls.Add("abort");
                return true;
            }
        }
    }
}
