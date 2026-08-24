using ShaPrint.Server;
using System;
using System.Collections.Generic;
using System.Threading;
using System.Threading.Tasks;
using Xunit;

namespace ShaPrint.Tests;

public sealed class ScannerServiceTests
{
    private static ScannerService CreateService(Func<string, int, int, string, CancellationToken, byte[]> worker)
        => new(worker, _ => new List<ShaPrint.Core.Network.ScannerInfo>());

    [Theory]
    [InlineData(0)]
    [InlineData(1201)]
    public async Task InvalidDpiIsRejectedBeforeWorkerStarts(int dpi)
    {
        int calls = 0;
        var service = CreateService((_, _, _, _, _) =>
        {
            Interlocked.Increment(ref calls);
            return new byte[] { 1 };
        });

        await Assert.ThrowsAsync<ArgumentOutOfRangeException>(() =>
            service.PerformScanAsync("Scanner A", dpi, 2, "JPEG"));

        Assert.Equal(0, calls);
    }

    [Fact]
    public async Task SameScannerIsRejectedImmediatelyButDifferentScannerCanRun()
    {
        using var started = new ManualResetEventSlim();
        using var release = new ManualResetEventSlim();
        int active = 0;
        int maximumActive = 0;
        var service = CreateService((_, _, _, _, _) =>
        {
            int current = Interlocked.Increment(ref active);
            InterlockedExtensions.Max(ref maximumActive, current);
            started.Set();
            release.Wait();
            Interlocked.Decrement(ref active);
            return new byte[] { 1 };
        });

        Task<byte[]> first = service.PerformScanAsync("Scanner A", 300, 2, "JPEG");
        Assert.True(started.Wait(TimeSpan.FromSeconds(2)));

        await Assert.ThrowsAsync<ScannerBusyException>(() =>
            service.PerformScanAsync("scanner a", 300, 2, "JPEG", timeout: TimeSpan.FromSeconds(1)));

        Task<byte[]> different = service.PerformScanAsync("Scanner B", 300, 2, "JPEG");
        Assert.True(started.Wait(TimeSpan.FromSeconds(2)));
        release.Set();

        await Task.WhenAll(first, different);
        Assert.Equal(2, maximumActive);
    }

    [Fact]
    public async Task HungWorkerTimesOutAndKeepsScannerAdmissionUntilWorkerExits()
    {
        using var started = new ManualResetEventSlim();
        using var release = new ManualResetEventSlim();
        using var finished = new ManualResetEventSlim();
        var service = CreateService((_, _, _, _, _) =>
        {
            started.Set();
            try
            {
                release.Wait();
                return new byte[] { 1 };
            }
            finally
            {
                finished.Set();
            }
        });

        Task<byte[]> timedOut = service.PerformScanAsync(
            "Scanner C", 300, 2, "JPEG", timeout: TimeSpan.FromMilliseconds(50));
        Assert.True(started.Wait(TimeSpan.FromSeconds(2)));
        await Assert.ThrowsAsync<TimeoutException>(() => timedOut);

        await Assert.ThrowsAsync<ScannerBusyException>(() =>
            service.PerformScanAsync("Scanner C", 300, 2, "JPEG", timeout: TimeSpan.FromMilliseconds(50)));

        release.Set();
        Assert.True(finished.Wait(TimeSpan.FromSeconds(2)));
        Assert.True(SpinWait.SpinUntil(
            () => !ScannerService.ActiveScans.ContainsKey("Scanner C"),
            TimeSpan.FromSeconds(2)));

        byte[] completed = await service.PerformScanAsync("Scanner C", 300, 2, "JPEG");
        Assert.Equal(new byte[] { 1 }, completed);
    }

    [Fact]
    public async Task CancellationIsPropagatedWithoutStartingWiaWork()
    {
        using var cancellation = new CancellationTokenSource();
        cancellation.Cancel();
        int calls = 0;
        var service = CreateService((_, _, _, _, token) =>
        {
            Interlocked.Increment(ref calls);
            token.ThrowIfCancellationRequested();
            return new byte[] { 1 };
        });

        await Assert.ThrowsAnyAsync<OperationCanceledException>(() =>
            service.PerformScanAsync("Scanner D", 300, 2, "JPEG", cancellation.Token));

        // The cancellation contract is checked before touching the WIA worker.
        Assert.Equal(0, calls);
    }

    [Fact]
    public async Task ScannerEnumerationHasADeadline()
    {
        using var release = new ManualResetEventSlim();
        var service = new ScannerService(
            enumerationWorker: _ =>
            {
                release.Wait();
                return new List<ShaPrint.Core.Network.ScannerInfo>();
            });

        var started = DateTime.UtcNow;
        var scanners = await service.GetLocalScannersAsync(timeout: TimeSpan.FromMilliseconds(50));

        Assert.Empty(scanners);
        Assert.True(DateTime.UtcNow - started < TimeSpan.FromSeconds(2));
        release.Set();
    }

    private static class InterlockedExtensions
    {
        public static void Max(ref int location, int value)
        {
            while (true)
            {
                int current = Volatile.Read(ref location);
                if (value <= current || Interlocked.CompareExchange(ref location, value, current) == current)
                    return;
            }
        }
    }
}
