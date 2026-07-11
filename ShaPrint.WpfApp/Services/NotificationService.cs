using System;
using Microsoft.Toolkit.Uwp.Notifications;
using ShaPrint.Core;

namespace ShaPrint.WpfApp.Services;

public class NotificationService : INotificationService
{
    public void ShowPrintJobCompleted(string documentName, string printerName)
    {
        try
        {
            new ToastContentBuilder()
                .AddText("Print Job Completed")
                .AddText($"{documentName} → {printerName}")
                .AddText(DateTime.Now.ToString("HH:mm:ss"))
                .Show();
        }
        catch (Exception ex)
        {
            AppLogger.Error($"[NOTIFICATION] Failed to show print job completed toast: {ex.Message}");
        }
    }

    public void ShowPrintJobFailed(string documentName, string printerName, string reason)
    {
        try
        {
            new ToastContentBuilder()
                .AddText("Print Job Failed")
                .AddText($"{documentName} → {printerName}")
                .AddText(reason)
                .Show();
        }
        catch (Exception ex)
        {
            AppLogger.Error($"[NOTIFICATION] Failed to show print job failed toast: {ex.Message}");
        }
    }

    public void ShowClientConnected(string clientAddress)
    {
        try
        {
            new ToastContentBuilder()
                .AddText("Client Connected")
                .AddText($"{clientAddress} connected")
                .AddText(DateTime.Now.ToString("HH:mm:ss"))
                .Show();
        }
        catch (Exception ex)
        {
            AppLogger.Error($"[NOTIFICATION] Failed to show client connected toast: {ex.Message}");
        }
    }

    public void ShowClientDisconnected(string clientAddress)
    {
        try
        {
            new ToastContentBuilder()
                .AddText("Client Disconnected")
                .AddText($"{clientAddress} disconnected")
                .AddText(DateTime.Now.ToString("HH:mm:ss"))
                .Show();
        }
        catch (Exception ex)
        {
            AppLogger.Error($"[NOTIFICATION] Failed to show client disconnected toast: {ex.Message}");
        }
    }

    public void ShowScanCompleted(string fileName)
    {
        try
        {
            new ToastContentBuilder()
                .AddText("Scan Complete")
                .AddText($"Saved to {fileName}")
                .AddText(DateTime.Now.ToString("HH:mm:ss"))
                .Show();
        }
        catch (Exception ex)
        {
            AppLogger.Error($"[NOTIFICATION] Failed to show scan completed toast: {ex.Message}");
        }
    }

    public void ShowScanFailed(string errorMessage)
    {
        try
        {
            new ToastContentBuilder()
                .AddText("Scan Failed")
                .AddText(errorMessage)
                .Show();
        }
        catch (Exception ex)
        {
            AppLogger.Error($"[NOTIFICATION] Failed to show scan failed toast: {ex.Message}");
        }
    }

    public void ShowPrinterError(string printerName, string errorDescription)
    {
        try
        {
            new ToastContentBuilder()
                .AddText("Printer Error")
                .AddText($"{printerName}: {errorDescription}")
                .Show();
        }
        catch (Exception ex)
        {
            AppLogger.Error($"[NOTIFICATION] Failed to show printer error toast: {ex.Message}");
        }
    }

    public void ShowSecurityAlert(string message, string detail)
    {
        try
        {
            new ToastContentBuilder()
                .AddText("Security Alert")
                .AddText(message)
                .AddText(detail)
                .Show();
        }
        catch (Exception ex)
        {
            AppLogger.Error($"[NOTIFICATION] Failed to show security alert toast: {ex.Message}");
        }
    }

    public void ShowToast(string title, string body, ToastAction? action = null)
    {
        try
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
        catch (Exception ex)
        {
            AppLogger.Error($"[NOTIFICATION] Failed to show general toast: {ex.Message}");
        }
    }
}
