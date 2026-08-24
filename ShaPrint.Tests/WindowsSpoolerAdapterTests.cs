using ShaPrint.Core.Ipp;
using System;
using System.Collections.Generic;
using System.Runtime.InteropServices;
using System.Threading;
using System.Threading.Tasks;
using Xunit;

namespace ShaPrint.Tests;

public sealed class WindowsSpoolerAdapterTests
{
    [Fact]
    public async Task PrintAsync_PartialWrite_ReturnsFailureAndClosesAllStartedResources()
    {
        var native = new FakeNative { BytesWritten = 2 };

        SpoolerResult result = await Create(native).PrintAsync(Job(), CancellationToken.None);

        Assert.False(result.Success);
        Assert.Equal(new[] { "open", "doc", "page", "write", "end-page", "end-doc", "close" }, native.Calls);
    }

    [Fact]
    public async Task PrintAsync_StartPageFailure_EndsDocumentAndClosesPrinter()
    {
        var native = new FakeNative { StartPageResult = false };

        SpoolerResult result = await Create(native).PrintAsync(Job(), CancellationToken.None);

        Assert.False(result.Success);
        Assert.Equal(new[] { "open", "doc", "page", "end-doc", "close" }, native.Calls);
    }

    [Fact]
    public async Task PrintAsync_WriteFailure_EndsPageAndDocumentAndClosesPrinter()
    {
        var native = new FakeNative { WriteResult = false, BytesWritten = 0 };

        SpoolerResult result = await Create(native).PrintAsync(Job(), CancellationToken.None);

        Assert.False(result.Success);
        Assert.Equal(new[] { "open", "doc", "page", "write", "end-page", "end-doc", "close" }, native.Calls);
    }

    [Fact]
    public async Task PrintAsync_Timeout_ReturnsPromptlyAndWorkerOwnsCleanupUntilNativeReturns()
    {
        using var entered = new ManualResetEventSlim();
        using var release = new ManualResetEventSlim();
        var native = new FakeNative { StartDocEntered = entered, StartDocRelease = release };

        Task<SpoolerResult> operation = Create(native, TimeSpan.FromMilliseconds(50)).PrintAsync(Job(), CancellationToken.None);
        Assert.True(entered.Wait(TimeSpan.FromSeconds(1)));

        SpoolerResult result = await operation;

        Assert.False(result.Success);
        Assert.Contains("timed out", result.ErrorMessage, StringComparison.OrdinalIgnoreCase);
        Assert.DoesNotContain("close", native.Calls);
        release.Set();
        Assert.True(SpinWait.SpinUntil(() => native.Calls.Contains("close"), TimeSpan.FromSeconds(1)));
    }

    [Fact]
    public async Task PrintAsync_Cancellation_ReturnsStableFailureWithoutStartingNativeWork()
    {
        using var cancellation = new CancellationTokenSource();
        cancellation.Cancel();
        var native = new FakeNative();

        SpoolerResult result = await Create(native).PrintAsync(Job(), cancellation.Token);

        Assert.False(result.Success);
        Assert.Contains("canceled", result.ErrorMessage, StringComparison.OrdinalIgnoreCase);
        Assert.Empty(native.Calls);
    }

    private static WindowsSpoolerAdapter Create(FakeNative native, TimeSpan? timeout = null)
        => new(native, timeout ?? TimeSpan.FromSeconds(1));

    private static PrintJob Job() => new() { PrinterName = "TestPrinter", Data = new byte[] { 1, 2, 3 }, DocumentName = "Test" };

    private sealed class FakeNative : IWindowsSpoolerNative
    {
        public List<string> Calls { get; } = new();
        public bool StartPageResult { get; init; } = true;
        public bool WriteResult { get; init; } = true;
        public int BytesWritten { get; init; } = 3;
        public ManualResetEventSlim? StartDocEntered { get; init; }
        public ManualResetEventSlim? StartDocRelease { get; init; }

        public bool OpenPrinter(string printerName, out IntPtr handle) { Calls.Add("open"); handle = new(1); return true; }
        public bool ClosePrinter(IntPtr handle) { Calls.Add("close"); return true; }
        public int StartDocPrinter(IntPtr handle, string documentName)
        {
            Calls.Add("doc");
            StartDocEntered?.Set();
            StartDocRelease?.Wait();
            return 42;
        }
        public bool EndDocPrinter(IntPtr handle) { Calls.Add("end-doc"); return true; }
        public bool StartPagePrinter(IntPtr handle) { Calls.Add("page"); return StartPageResult; }
        public bool EndPagePrinter(IntPtr handle) { Calls.Add("end-page"); return true; }
        public bool WritePrinter(IntPtr handle, IntPtr bytes, int count, out int written) { Calls.Add("write"); written = BytesWritten; return WriteResult; }
        public bool SetJob(IntPtr handle, int jobId) { Calls.Add("abort"); return true; }
    }
}
