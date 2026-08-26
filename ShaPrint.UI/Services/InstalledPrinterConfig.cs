namespace ShaPrint.UI.Services;

/// <summary>
/// Persisted config for one installed ShaPrint virtual printer (client side).
/// Migrated from <c>ShaPrint.WpfApp/ViewModels/Pages/ClientViewModel.cs</c> (Task 4) so the
/// shared <see cref="ServerReachabilityTracker"/> can be platform-agnostic. The WPF copy is
/// kept in <c>ShaPrint.WpfApp</c> until cutover (Task 5+).
/// </summary>
public class InstalledPrinterConfig
{
    public string VirtualPrinterName { get; set; } = string.Empty;
    public string PipeName { get; set; } = string.Empty;
    public string ServerIp { get; set; } = string.Empty;
    public string TargetPrinterName { get; set; } = string.Empty;
    public string DriverName { get; set; } = string.Empty;

    /// <summary>
    /// Stable per-server UUID captured from the discovery response at install time.
    /// Null for installs from pre-ServerId servers. Used by ServerReachabilityTracker
    /// to match this entry against a discovered server whose IP may have changed.
    /// </summary>
    public string? ServerId { get; set; }
}