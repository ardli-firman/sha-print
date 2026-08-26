namespace ShaPrint.Platform.Abstractions;

public record ToastAction(string ActivationType, string Arguments);

public interface INotificationService
{
    void ShowToast(string title, string body, ToastAction? action = null);
    void ShowPrintJobCompleted(string documentName, string printerName);
    void ShowPrintJobFailed(string documentName, string printerName, string reason);
    void ShowClientConnected(string clientAddress);
    void ShowClientDisconnected(string clientAddress);
    void ShowScanCompleted(string fileName);
    void ShowScanFailed(string errorMessage);
    void ShowPrinterError(string printerName, string errorDescription);
    void ShowSecurityAlert(string message, string detail);
}