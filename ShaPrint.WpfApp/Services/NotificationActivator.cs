using System.IO;
using System.IO.Pipes;
using System.Runtime.InteropServices;
using Microsoft.Toolkit.Uwp.Notifications;

namespace ShaPrint.WpfApp.Services;

/// <summary>
/// COM-visible activator that handles toast notification click events.
/// Registered via <c>DesktopNotificationManagerCompat.RegisterActivator()</c> in Task 3.
/// </summary>
#pragma warning disable CS0618 // NotificationActivator is obsolete per toolkit recommendation; COM approach required for DesktopNotificationManagerCompat.RegisterActivator<T>()
[ClassInterface(ClassInterfaceType.None)]
[ComVisible(true)]
[Guid("0B640367-F3EC-4F81-AF86-B648234F059A")]
public class NotificationActivator : Microsoft.Toolkit.Uwp.Notifications.NotificationActivator
{
    private static string? _lastActivationArguments;
    private static readonly object _activationLock = new();

    /// <summary>
    /// Gets the most recent activation arguments (thread-safe snapshot).
    /// </summary>
    public static string? LastActivationArguments
    {
        get { lock (_activationLock) return _lastActivationArguments; }
    }

    public override void OnActivated(string arguments, NotificationUserInput userInput, string appUserModelId)
    {
        // Store activation args for the main app to pick up
        lock (_activationLock)
        {
            _lastActivationArguments = arguments;
        }

        System.Diagnostics.Debug.WriteLine($"[ACTIVATOR] Activated: {arguments}");

        // Try to forward activation args to running app via named pipe without blocking the COM STA thread
        _ = System.Threading.Tasks.Task.Run(async () =>
        {
            try
            {
                using var client = new NamedPipeClientStream(".", "ShaPrint.ActivationPipe", PipeDirection.Out);
                await client.ConnectAsync(1000);
                using var writer = new StreamWriter(client);
                await writer.WriteLineAsync(arguments);
                await writer.FlushAsync();
            }
            catch (Exception ex)
            {
                // Pipe write failed — app may not be running or pipe not yet created; safe to ignore
                System.Diagnostics.Debug.WriteLine($"[ACTIVATOR] Pipe write failed: {ex.Message}");
            }
        });
    }
}
#pragma warning restore CS0618
