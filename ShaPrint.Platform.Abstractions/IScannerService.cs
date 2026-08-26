using ShaPrint.Core.Network;

namespace ShaPrint.Platform.Abstractions;

public interface IScannerService
{
    List<ScannerInfo> GetLocalScanners();
    byte[] PerformScan(string scannerName, int dpi, int colorMode, string format, out string actualFormat);
}