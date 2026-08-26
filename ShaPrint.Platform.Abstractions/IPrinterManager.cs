using ShaPrint.Core.Network;

namespace ShaPrint.Platform.Abstractions;

public interface IPrinterManager
{
    Task<List<PrinterInfo>> GetLocalPrintersAsync();
    Task<bool> PrintRawDataAsync(string printerName, byte[] data, string documentName, TimeSpan? timeout = null);
}