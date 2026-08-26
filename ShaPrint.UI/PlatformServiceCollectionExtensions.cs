using Microsoft.Extensions.DependencyInjection;
using ShaPrint.Platform.Abstractions;
using ShaPrint.UI.Services;
using ShaPrint.UI.ViewModels;
using ShaPrint.UI.ViewModels.Pages;
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
        services.AddSharedServices();
        services.AddViewModels();
#if WINDOWS
        services.AddSingleton<IPrinterManager, WindowsPrinterManager>();
        services.AddSingleton<IVirtualPrinterManager, WindowsVirtualPrinterManager>();
        services.AddSingleton<IScannerService, WindowsScannerService>();
        services.AddSingleton<IStartupManager, WindowsStartupManager>();
        services.AddSingleton<INotificationService, WindowsNotificationService>();
        services.AddSingleton<IFirewallManager, WindowsFirewallManager>();
        services.AddSingleton<IPrintRelayClient, WindowsPrintRelayClient>();

        // Server-side monitor + driver sharing (Gap-closing task): the spooler queue probe and
        // auto-purge monitor are Windows-only (System.Printing); SystemDelayProbe is
        // platform-agnostic but only consumed by the monitor, so it is registered alongside it.
        services.AddSingleton<IDriverPackageProvider, WindowsDriverPackageProvider>();
        services.AddSingleton<IPrintQueueProbe, LocalPrintQueueProbe>();
        services.AddSingleton<IDelayProbe, SystemDelayProbe>();
        services.AddSingleton<PrintMonitorService>(sp => new PrintMonitorService(
            sp.GetRequiredService<INotificationService>(),
            sp.GetRequiredService<IPrintQueueProbe>(),
            sp.GetRequiredService<IDelayProbe>()));
#endif
        return services;
    }

    /// <summary>
    /// Unix (macOS/Linux) backend registrations. Stub for now — Task 7 akan mengisi
    /// implementasi Unix (CUPS lpstat/lp, SANE scanimage, LaunchAgent/systemd, osascript/
    /// notify-send, pfctl/ufw). Must compile under net8.0, so it must NOT reference the
    /// (not yet created) ShaPrint.Platform.Unix project. Shared services still register so
    /// the app shell is consistent on every platform.
    /// </summary>
    public static IServiceCollection AddPlatformUnix(this IServiceCollection services)
    {
        services.AddSharedServices();
        services.AddViewModels();
        // Task 7 akan mengisi implementasi Unix.
        return services;
    }

    /// <summary>
    /// Shared (platform-agnostic) services migrated from ShaPrint.WpfApp in Task 4. They only
    /// depend on <c>ShaPrint.Core</c> and <c>ShaPrint.Platform.Abstractions</c> types.
    ///
    /// <para><see cref="ServerReachabilityTracker"/> is deliberately NOT registered here: its
    /// collaborators are per-client-instance delegates (config provider, discovery scanner,
    /// identity-change callbacks) supplied by the ClientViewModel — a DI factory can't provide
    /// them. The client ViewModel constructs it directly, mirroring ShaPrint.WpfApp.</para>
    /// </summary>
    public static IServiceCollection AddSharedServices(this IServiceCollection services)
    {
        // Networking / server engines (stateful singletons).
        services.AddSingleton<DiscoveryClientService>();
        services.AddSingleton<DiscoveryServerService>();
        services.AddSingleton<PrintReceiverService>();

        // Monitor + client-side helpers.
        services.AddSingleton<MonitorService>();
        services.AddSingleton<ScanClientService>();

        // Print relay orchestration (used by CLI `send` + backend script path, Task 8).
        services.AddSingleton<PrintRelayClientService>();

        // GitHub-based auto-update: singleton + hosted background check (same split as WpfApp).
        services.AddSingleton<UpdateService>();
        services.AddHostedService(provider => provider.GetRequiredService<UpdateService>());

        return services;
    }

    /// <summary>
    /// ViewModel registrations (Task 5). Lifetimes mirror ShaPrint.WpfApp's App.xaml.cs:
    /// the shell and long-lived page engines are singletons; Welcome and Updates are transient.
    ///
    /// <para>Page ViewModels that touch platform backends (<c>ServerViewModel</c>,
    /// <c>ClientViewModel</c>, <c>ScanViewModel</c>, <c>SettingsViewModel</c>) take
    /// <c>IServiceProvider</c> and resolve those interfaces lazily through
    /// <see cref="ViewModelSupport.Resolve{T}"/>, so constructing them is safe on every platform
    /// (before Task 7 a macOS/Linux build has no Windows-only backends registered).</para>
    /// </summary>
    public static IServiceCollection AddViewModels(this IServiceCollection services)
    {
        services.AddSingleton<MainWindowViewModel>();
        services.AddTransient<WelcomeViewModel>();
        services.AddSingleton<ServerViewModel>();
        services.AddSingleton<ClientViewModel>();
        services.AddSingleton<MonitorViewModel>();
        services.AddSingleton<ScanViewModel>();
        services.AddSingleton<SettingsViewModel>();
        services.AddTransient<UpdatesViewModel>();

        // Feed the persisted UI settings (Models.AppSettings) into UpdateService's background
        // auto-check (channel, auto-update toggle, last-check gate).
        services.AddSingleton<Func<UpdateCheckSettings>>(_ => () =>
            new UpdateCheckSettings(
                Models.AppSettings.Current.Channel,
                Models.AppSettings.Current.AutoUpdateEnabled,
                Models.AppSettings.Current.LastUpdateCheck));

        return services;
    }
}