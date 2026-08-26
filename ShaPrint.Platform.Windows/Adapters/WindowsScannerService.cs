using ShaPrint.Core.Network;
using ShaPrint.Platform.Abstractions;

namespace ShaPrint.Platform.Windows.Adapters;

/// <summary>
/// Adapter exposing the WIA-based <see cref="ScannerService"/> through the
/// platform-agnostic <see cref="IScannerService"/> interface.
/// </summary>
public sealed class WindowsScannerService : IScannerService
{
    private readonly ScannerService _inner = new();

    public List<ScannerInfo> GetLocalScanners()
        => _inner.GetLocalScanners();

    public byte[] PerformScan(string scannerName, int dpi, int colorMode, string format, out string actualFormat)
        => _inner.PerformScan(scannerName, dpi, colorMode, format, out actualFormat);
}