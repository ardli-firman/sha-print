using Microsoft.Extensions.DependencyInjection;
using ShaPrint.Platform.Abstractions;
#if WINDOWS
using ShaPrint.Platform.Windows.Adapters;
#endif

namespace ShaPrint.UI;

/// <summary>
/// Platform DI registration used by the runtime switch in Program.ConfigureServices.
/// </summary>
public static class PlatformServiceCollectionExtensions
{
    /// <summary>
    /// Registers the real Windows backends (ShaPrint.Platform.Windows adapters).
    ///
    /// The registration body is compiled only for the net8.0-windows10.0.17763 TFM
    /// (#if WINDOWS): the plain net8.0 TFM cannot reference ShaPrint.Platform.Windows, so its
    /// types must not appear there. At runtime AddPlatformWindows is only invoked when
    /// OperatingSystem.IsWindows() is true, and the Windows build is the one shipped on
    /// Windows — so the empty net8.0 body is unreachable in the supported deployment paths.
    /// </summary>
    public static IServiceCollection AddPlatformWindows(this IServiceCollection services)
    {
#if WINDOWS
        services.AddSingleton<IPrinterManager, WindowsPrinterManager>();
        services.AddSingleton<IVirtualPrinterManager, WindowsVirtualPrinterManager>();
        services.AddSingleton<IScannerService, WindowsScannerService>();
        services.AddSingleton<IStartupManager, WindowsStartupManager>();
        services.AddSingleton<INotificationService, WindowsNotificationService>();
        services.AddSingleton<IFirewallManager, WindowsFirewallManager>();
        services.AddSingleton<IPrintRelayClient, WindowsPrintRelayClient>();
#endif
        return services;
    }

    /// <summary>
    /// Unix (macOS/Linux) backend registrations. Stub for now — Task 7 akan mengisi
    /// implementasi Unix (CUPS lpstat/lp, SANE scanimage, LaunchAgent/systemd, osascript/
    /// notify-send, pfctl/ufw). Must compile under net8.0, so it must NOT reference the
    /// (not yet created) ShaPrint.Platform.Unix project.
    /// </summary>
    public static IServiceCollection AddPlatformUnix(this IServiceCollection services)
    {
        // Task 7 akan mengisi implementasi Unix.
        return services;
    }
}