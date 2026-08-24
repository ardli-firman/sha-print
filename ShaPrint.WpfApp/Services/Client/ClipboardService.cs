using System;
using System.Threading;
using System.Threading.Tasks;
using System.Windows;

namespace ShaPrint.WpfApp.Services.Client;

public interface IClipboardService
{
    Task<bool> TrySetTextAsync(string text, CancellationToken cancellationToken = default);
}

public sealed class ClipboardService : IClipboardService
{
    public const int ClipboardBusyHResult = unchecked((int)0x800401D0);

    private readonly Action<string> _setText;
    private readonly Func<TimeSpan, Task> _delayAsync;
    private readonly int _maxAttempts;
    private readonly TimeSpan _retryDelay;

    public ClipboardService()
        : this(Clipboard.SetText, Task.Delay)
    {
    }

    internal ClipboardService(
        Action<string> setText,
        Func<TimeSpan, Task> delayAsync,
        int maxAttempts = 5,
        TimeSpan? retryDelay = null)
    {
        _setText = setText ?? throw new ArgumentNullException(nameof(setText));
        _delayAsync = delayAsync ?? throw new ArgumentNullException(nameof(delayAsync));
        _maxAttempts = Math.Max(1, maxAttempts);
        _retryDelay = retryDelay ?? TimeSpan.FromMilliseconds(50);
    }

    public async Task<bool> TrySetTextAsync(string text, CancellationToken cancellationToken = default)
    {
        ArgumentNullException.ThrowIfNull(text);

        for (int attempt = 1; attempt <= _maxAttempts; attempt++)
        {
            cancellationToken.ThrowIfCancellationRequested();
            try
            {
                _setText(text);
                return true;
            }
            catch (Exception ex) when (ex.HResult == ClipboardBusyHResult)
            {
                if (attempt == _maxAttempts)
                    return false;

                await _delayAsync(_retryDelay);
            }
            catch (Exception)
            {
                // Clipboard access can also fail when invoked outside the WPF
                // STA. Let the caller show a manual-copy fallback instead of
                // turning a convenience action into a command failure.
                return false;
            }
        }

        return false;
    }
}
