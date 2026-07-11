using ShaPrint.WpfApp.Services;

namespace ShaPrint.Tests;

public class NotificationServiceTests
{
    [Fact]
    public void ShowPrintJobCompleted_DoesNotThrow()
    {
        var service = new NotificationService();
        var ex = Record.Exception(() =>
            service.ShowPrintJobCompleted("doc.pdf", "Epson L3210"));
        Assert.Null(ex);
    }

    [Fact]
    public void ShowClientConnected_DoesNotThrow()
    {
        var service = new NotificationService();
        var ex = Record.Exception(() =>
            service.ShowClientConnected("192.168.1.42"));
        Assert.Null(ex);
    }

    [Fact]
    public void ShowClientDisconnected_DoesNotThrow()
    {
        var service = new NotificationService();
        var ex = Record.Exception(() =>
            service.ShowClientDisconnected("192.168.1.42"));
        Assert.Null(ex);
    }

    [Fact]
    public void ShowPrinterError_DoesNotThrow()
    {
        var service = new NotificationService();
        var ex = Record.Exception(() =>
            service.ShowPrinterError("Epson L3210", "Paper jam detected"));
        Assert.Null(ex);
    }
}
