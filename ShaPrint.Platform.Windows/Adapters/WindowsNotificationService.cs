using AbstractionsNotificationService = ShaPrint.Platform.Abstractions.INotificationService;
using AbstractionsToastAction = ShaPrint.Platform.Abstractions.ToastAction;

namespace ShaPrint.Platform.Windows.Adapters;

/// <summary>
/// Adapter exposing the WPF-era <see cref="NotificationService"/> through the
/// platform-abstraction <c>INotificationService</c> AND the legacy Windows interface
/// (which moved here together with <see cref="NotificationService"/>).
///
/// The same instance can be resolved from DI under either interface; behavior is
/// identical to the original WPF registration (toast content + error handling unchanged).
/// </summary>
public sealed class WindowsNotificationService : ShaPrint.Platform.Windows.INotificationService, AbstractionsNotificationService
{
    private readonly NotificationService _inner;

    public WindowsNotificationService()
    {
        _inner = new NotificationService();
    }

    public void ShowPrintJobCompleted(string documentName, string printerName)
        => _inner.ShowPrintJobCompleted(documentName, printerName);

    public void ShowPrintJobFailed(string documentName, string printerName, string reason)
        => _inner.ShowPrintJobFailed(documentName, printerName, reason);

    public void ShowClientConnected(string clientAddress)
        => _inner.ShowClientConnected(clientAddress);

    public void ShowClientDisconnected(string clientAddress)
        => _inner.ShowClientDisconnected(clientAddress);

    public void ShowScanCompleted(string fileName)
        => _inner.ShowScanCompleted(fileName);

    public void ShowScanFailed(string errorMessage)
        => _inner.ShowScanFailed(errorMessage);

    public void ShowPrinterError(string printerName, string errorDescription)
        => _inner.ShowPrinterError(printerName, errorDescription);

    public void ShowSecurityAlert(string message, string detail)
        => _inner.ShowSecurityAlert(message, detail);

    // Windows (legacy consumer) flavor — same record, direct delegation.
    public void ShowToast(string title, string body, ShaPrint.Platform.Windows.ToastAction? action = null)
        => _inner.ShowToast(title, body, action);

    // Abstractions flavor — maps the record, then delegates to the same toast path.
    public void ShowToast(string title, string body, AbstractionsToastAction? action = null)
        => _inner.ShowToast(title, body,
            action is null ? null : new ShaPrint.Platform.Windows.ToastAction(action.ActivationType, action.Arguments));
}