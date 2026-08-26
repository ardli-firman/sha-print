using System.Text;
using ShaPrint.Platform.Abstractions;

namespace ShaPrint.Platform.Windows.Adapters;

/// <summary>
/// Adapter exposing the existing <see cref="VirtualPrinterManager"/> provisioning
/// pipeline through <see cref="IVirtualPrinterManager"/>.
///
/// The Win32 <see cref="VirtualPrinterManager"/> API requires an explicit pipe name;
/// this adapter derives it deterministically from the virtual printer name
/// (<c>\\.\pipe\ShaPrint_&lt;sanitized-name&gt;</c>) so callers never pass it.
/// Install/remove use the SAME derived name, keeping the printer's port in sync.
/// </summary>
public sealed class WindowsVirtualPrinterManager : IVirtualPrinterManager
{
    /// <summary>
    /// Derives the named-pipe port name used for a virtual printer,
    /// e.g. "ShaPrint [PC-01] - Epson L3210" -> "\\.\pipe\ShaPrint_ShaPrint__PC-01___Epson_L3210".
    /// </summary>
    internal static string DerivePipeName(string virtualPrinterName)
        => $@"\\.\pipe\ShaPrint_{Sanitize(virtualPrinterName)}";

    private static string Sanitize(string name)
    {
        var sb = new StringBuilder(name.Length);
        foreach (char c in name)
        {
            sb.Append(char.IsLetterOrDigit(c) || c is '-' or '_' or '.' ? c : '_');
        }
        return sb.ToString();
    }

    public Task<(bool Success, string ErrorMessage)> InstallPrinterAsync(string virtualPrinterName, string driverName)
        => VirtualPrinterManager.InstallPrinterAsync(virtualPrinterName, DerivePipeName(virtualPrinterName), driverName);

    public Task<(bool Success, string ErrorMessage)> RemovePrinterAsync(string virtualPrinterName)
        => VirtualPrinterManager.RemovePrinterAsync(virtualPrinterName, DerivePipeName(virtualPrinterName));

    public bool CheckPrinterExists(string printerName)
        => VirtualPrinterManager.CheckPrinterExists(printerName);

    public List<string> GetInstalledDrivers()
        => VirtualPrinterManager.GetInstalledDrivers();

    /// <summary>
    /// Enumerates installed virtual printers using the established ShaPrint naming
    /// convention ("ShaPrint [...]" / "ShaPrint - ...") via the Win32 spooler.
    /// </summary>
    public List<string> GetInstalledVirtualPrinters()
        => SpoolerApi.GetLocalPrinters()
            .Where(n => n.StartsWith("ShaPrint ", StringComparison.OrdinalIgnoreCase))
            .OrderBy(n => n, StringComparer.OrdinalIgnoreCase)
            .ToList();
}