using System;
using System.Runtime.InteropServices;
using System.Threading.Tasks;
using ShaPrint.WpfApp.Services.Client;
using Xunit;

namespace ShaPrint.Tests;

public sealed class ClipboardServiceTests
{
    [Fact]
    public async Task RetriesTransientClipboardLockAndEventuallySucceeds()
    {
        int attempts = 0;
        int delays = 0;
        var service = new ClipboardService(
            setText: _ =>
            {
                attempts++;
                if (attempts < 3)
                    throw new ExternalException("clipboard is busy", ClipboardService.ClipboardBusyHResult);
            },
            delayAsync: _ =>
            {
                delays++;
                return Task.CompletedTask;
            },
            maxAttempts: 4,
            retryDelay: TimeSpan.Zero);

        bool copied = await service.TrySetTextAsync("ipp://server/printer");

        Assert.True(copied);
        Assert.Equal(3, attempts);
        Assert.Equal(2, delays);
    }

    [Fact]
    public async Task ReturnsFalseAfterPersistentClipboardLockWithoutThrowing()
    {
        int attempts = 0;
        var service = new ClipboardService(
            setText: _ =>
            {
                attempts++;
                throw new ExternalException("clipboard is busy", ClipboardService.ClipboardBusyHResult);
            },
            delayAsync: _ => Task.CompletedTask,
            maxAttempts: 3,
            retryDelay: TimeSpan.Zero);

        bool copied = await service.TrySetTextAsync("ipp://server/printer");

        Assert.False(copied);
        Assert.Equal(3, attempts);
    }

    [Fact]
    public async Task DoesNotRetryNonClipboardErrors()
    {
        int attempts = 0;
        var service = new ClipboardService(
            setText: _ =>
            {
                attempts++;
                throw new InvalidOperationException("clipboard called off STA thread");
            },
            delayAsync: _ => Task.CompletedTask,
            maxAttempts: 4,
            retryDelay: TimeSpan.Zero);

        bool copied = await service.TrySetTextAsync("ipp://server/printer");

        Assert.False(copied);
        Assert.Equal(1, attempts);
    }
}
