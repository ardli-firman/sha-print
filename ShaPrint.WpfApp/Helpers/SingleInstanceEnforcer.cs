using System.Diagnostics;
using System.Threading;

namespace ShaPrint.WpfApp.Helpers;

/// <summary>
/// Ensures only one instance of the application runs at any time.
/// 
/// Uses a named <see cref="Mutex"/> (kernel-level) for instance detection
/// and a named <see cref="EventWaitHandle"/> for cross-process signaling.
/// 
/// When a second instance attempts to start:
///   1. It detects the mutex is already held
///   2. It signals the first instance via the event handle
///   3. It returns <c>false</c> so the caller can exit immediately
/// 
/// The first instance listens for activation signals on a background thread
/// and invokes the provided callback (e.g. to bring the window to front).
/// </summary>
public sealed class SingleInstanceEnforcer : IDisposable
{
    private readonly string _mutexName;
    private readonly string _activateEventName;
    private readonly Action _onActivate;

    private Mutex? _mutex;
    private EventWaitHandle? _activateEvent;
    private Thread? _listenerThread;
    private bool _disposed;

    /// <summary>
    /// Whether this instance successfully acquired the mutex and is the
    /// first/primary application instance.
    /// </summary>
    public bool IsFirstInstance { get; private set; }

    /// <summary>
    /// Whether this instance has been disposed.
    /// </summary>
    public bool Disposed => _disposed;

    /// <summary>
    /// The application GUID used to build the kernel-object names.
    /// Exposed so tests can reuse the same constant.
    /// </summary>
    public const string AppGuid = "d4f7c9a1-3b5e-4f2a-8c7d-9e1b3a5c7f9d";

    /// <summary>
    /// Full mutex name (used in kernel namespace).
    /// </summary>
    public const string MutexName = $"Global\\ShaPrint_{AppGuid}";

    /// <summary>
    /// Full event-handle name (used in kernel namespace).
    /// </summary>
    public const string ActivateEventName = $"Global\\ShaPrint_{AppGuid}_Activate";

    /// <summary>
    /// Creates an enforcer with the default application identifier.
    /// </summary>
    /// <param name="onActivate">
    /// Callback invoked on the listener thread when a second-instance
    /// activation signal is received. Must dispatch to the UI thread
    /// if it touches WPF objects.
    /// </param>
    public SingleInstanceEnforcer(Action onActivate)
        : this(AppGuid, onActivate)
    {
    }

    /// <summary>
    /// Creates an enforcer with a custom application identifier
    /// (primarily for testing with isolated names).
    /// </summary>
    internal SingleInstanceEnforcer(string appGuid, Action onActivate)
    {
        ArgumentException.ThrowIfNullOrWhiteSpace(appGuid);
        ArgumentNullException.ThrowIfNull(onActivate);

        _mutexName = $"Global\\ShaPrint_{appGuid}";
        _activateEventName = $"Global\\ShaPrint_{appGuid}_Activate";
        _onActivate = onActivate;
    }

    /// <summary>
    /// Attempts to acquire the single-instance mutex.
    /// </summary>
    /// <returns>
    /// <c>true</c> if this is the first (primary) instance;
    /// <c>false</c> if another instance is already running.
    /// When <c>false</c>, the caller should exit immediately.
    /// </returns>
    public bool TryAcquire()
    {
        ObjectDisposedException.ThrowIf(_disposed, this);

        var mutex = new Mutex(initiallyOwned: true, _mutexName, out bool createdNew);

        if (!createdNew)
        {
            // Another instance owns the mutex.
            // Signal it to bring its window to the foreground, then exit.
            mutex.Dispose();

            try
            {
                using var signal = new EventWaitHandle(false, EventResetMode.AutoReset, _activateEventName);
                signal.Set();
            }
            catch
            {
                // Best-effort: if signaling fails (e.g. first instance
                // already exited), the user can manually switch.
            }

            IsFirstInstance = false;
            return false;
        }

        // We own the mutex — this is the first instance.
        _mutex = mutex;
        IsFirstInstance = true;

        // Start the background listener for activation signals.
        _activateEvent = new EventWaitHandle(false, EventResetMode.AutoReset, _activateEventName);
        _listenerThread = new Thread(ListenerLoop)
        {
            IsBackground = true,
            Name = "ShaPrint-ActivationListener"
        };
        _listenerThread.Start();

        return true;
    }

    /// <summary>
    /// Background-thread loop that waits for activation signals from
    /// second-instance attempts.
    /// </summary>
    private void ListenerLoop()
    {
        var ev = _activateEvent;
        while (ev != null && !_disposed)
        {
            try
            {
                ev.WaitOne();
            }
            catch
            {
                break;
            }

            if (!_disposed)
            {
                _onActivate();
            }
        }
    }

    public void Dispose()
    {
        if (_disposed) return;
        _disposed = true;

        // Unblock the listener thread so it can exit.
        try { _activateEvent?.Set(); } catch { }
        try { _activateEvent?.Dispose(); } catch { }
        _activateEvent = null;

        // Release the mutex so a future instance can start.
        try
        {
            _mutex?.ReleaseMutex();
            _mutex?.Dispose();
        }
        catch
        {
            // OS will release abandoned mutex on process exit.
        }
        _mutex = null;

        // Allow the listener thread to finish naturally.
        // It's a background thread so it won't keep the process alive.
        _listenerThread = null;
    }
}
