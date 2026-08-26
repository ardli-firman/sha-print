using ShaPrint.Core.Network;
using ShaPrint.Platform.Abstractions;

namespace ShaPrint.Platform.Windows.Adapters;

/// <summary>
/// Adapter exposing the Win32 spooler API (<see cref="SpoolerApi"/>) through the
/// platform-agnostic <see cref="IPrinterManager"/> interface.
/// </summary>
public sealed class WindowsPrinterManager : IPrinterManager
{
    public Task<List<PrinterInfo>> GetLocalPrintersAsync()
        => Task.FromResult(SpoolerApi.GetLocalPrintersDetailed());

    public Task<bool> PrintRawDataAsync(string printerName, byte[] data, string documentName, TimeSpan? timeout = null)
        => SpoolerApi.PrintRawDataAsync(printerName, data, documentName, timeout);
}