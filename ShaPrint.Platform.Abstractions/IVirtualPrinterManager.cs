namespace ShaPrint.Platform.Abstractions;

public interface IVirtualPrinterManager
{
    Task<(bool Success, string ErrorMessage)> InstallPrinterAsync(string virtualPrinterName, string driverName);
    Task<(bool Success, string ErrorMessage)> RemovePrinterAsync(string virtualPrinterName);
    bool CheckPrinterExists(string printerName);
    List<string> GetInstalledDrivers();
    List<string> GetInstalledVirtualPrinters();
}