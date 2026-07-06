using Microsoft.Toolkit.Uwp.Notifications;

namespace ShaPrint.WpfApp.Services;

public class NotificationService : INotificationService
{
    public void ShowPrintJobCompleted(string documentName, string printerName)
    {
        new ToastContentBuilder()
            .AddText("Print Job Completed")
            .AddText($"{documentName} → {printerName}")
            .AddText(DateTime.Now.ToString("HH:mm:ss"))
            .Show();
    }

    public void ShowPrintJobFailed(string documentName, string printerName, string reason)
    {
        new ToastContentBuilder()
            .AddText("Print Job Failed")
            .AddText($"{documentName} → {printerName}")
            .AddText(reason)
            .Show();
    }

    public void ShowClientConnected(string clientAddress)
    {
        new ToastContentBuilder()
            .AddText("Client Connected")
            .AddText($"{clientAddress} connected")
            .AddText(DateTime.Now.ToString("HH:mm:ss"))
            .Show();
    }

    public void ShowClientDisconnected(string clientAddress)
    {
        new ToastContentBuilder()
            .AddText("Client Disconnected")
            .AddText($"{clientAddress} disconnected")
            .AddText(DateTime.Now.ToString("HH:mm:ss"))
            .Show();
    }

    public void ShowScanCompleted(string fileName)
    {
        new ToastContentBuilder()
            .AddText("Scan Complete")
            .AddText($"Saved to {fileName}")
            .AddText(DateTime.Now.ToString("HH:mm:ss"))
            .Show();
    }

    public void ShowScanFailed(string errorMessage)
    {
        new ToastContentBuilder()
            .AddText("Scan Failed")
            .AddText(errorMessage)
            .Show();
    }

    public void ShowPrinterError(string printerName, string errorDescription)
    {
        new ToastContentBuilder()
            .AddText("Printer Error")
            .AddText($"{printerName}: {errorDescription}")
            .Show();
    }

    public void ShowSecurityAlert(string message, string detail)
    {
        new ToastContentBuilder()
            .AddText("Security Alert")
            .AddText(message)
            .AddText(detail)
            .Show();
    }

    public void ShowToast(string title, string body, ToastAction? action = null)
    {
        var builder = new ToastContentBuilder()
            .AddText(title)
            .AddText(body);

        if (action != null)
        {
            builder.AddArgument("action", action.Arguments);
        }

        builder.Show();
    }
}
