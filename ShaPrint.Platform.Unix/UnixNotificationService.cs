using ShaPrint.Core;
using ShaPrint.Platform.Abstractions;

namespace ShaPrint.Platform.Unix;

/// <summary>
/// <see cref="INotificationService"/> for macOS/Linux.
///
/// macOS: <c>osascript -e 'display notification ...'</c> (strings are escaped for
/// AppleScript — no shell is involved, <see cref="UnixProcessRunner"/> passes the script as
/// a single argument). Linux: <c>notify-send &lt;title&gt; &lt;body&gt;</c> (libnotify).
///
/// Notifications are best-effort: a missing/unsupported tool is logged, never thrown.
/// <see cref="ToastAction"/> has no activation semantics in these channels (osascript /
/// notify-send have no click handler), so it is accepted and ignored on Unix.
/// </summary>
public sealed class UnixNotificationService : INotificationService
{
    public void ShowToast(string title, string body, ToastAction? action = null)
    {
        if (action != null)
        {
            AppLogger.Log($"[NOTIFY] ToastAction ({action.ActivationType}, {action.Arguments}) ignored — no activation channel on Unix.");
        }

        try
        {
            if (OperatingSystem.IsMacOS())
            {
                ShowMacOs(title, body);
            }
            else if (OperatingSystem.IsLinux())
            {
                ShowLinux(title, body);
            }
            else
            {
                AppLogger.Log($"[NOTIFY] {title}: {body}");
            }
        }
        catch (Exception ex)
        {
            AppLogger.Error("[NOTIFY] Failed to show toast: " + ex.Message);
        }
    }

    public void ShowPrintJobCompleted(string documentName, string printerName)
        => ShowToast("Print job completed", $"{documentName} → {printerName}");

    public void ShowPrintJobFailed(string documentName, string printerName, string reason)
        => ShowToast("Print job failed", $"{documentName} → {printerName}: {reason}");

    public void ShowClientConnected(string clientAddress)
        => ShowToast("Client connected", clientAddress);

    public void ShowClientDisconnected(string clientAddress)
        => ShowToast("Client disconnected", clientAddress);

    public void ShowScanCompleted(string fileName)
        => ShowToast("Scan completed", fileName);

    public void ShowScanFailed(string errorMessage)
        => ShowToast("Scan failed", errorMessage);

    public void ShowPrinterError(string printerName, string errorDescription)
        => ShowToast($"Printer error: {printerName}", errorDescription);

    public void ShowSecurityAlert(string message, string detail)
        => ShowToast($"Security alert: {message}", detail);

    // ── macOS: osascript ─────────────────────────────────────────────────────────

    private static void ShowMacOs(string title, string body)
    {
        // One AppleScript expression; quotes inside title/body are escaped for AppleScript
        // and the whole script is passed as a single ArgumentList element (no shell parsing).
        string script =
            $"display notification {ToAppleScriptString(body)} with title {ToAppleScriptString(title)}";

        var result = UnixProcessRunner.Run("osascript", new[] { "-e", script });
        if (!result.Succeeded)
        {
            AppLogger.Error($"[NOTIFY] osascript failed (exit {result.ExitCode}): {result.StdErr.Trim()}");
        }
    }

    private static string ToAppleScriptString(string value)
        => "\"" + value.Replace("\\", "\\\\").Replace("\"", "\\\"") + "\"";

    // ── Linux: notify-send ───────────────────────────────────────────────────────

    private static void ShowLinux(string title, string body)
    {
        var result = UnixProcessRunner.Run("notify-send", new[] { title, body });
        if (!result.Succeeded)
        {
            AppLogger.Error($"[NOTIFY] notify-send failed (exit {result.ExitCode}): {result.StdErr.Trim()} " +
                            "— install libnotify (e.g. `sudo apt install libnotify-bin`).");
        }
    }
}
