using Microsoft.Extensions.DependencyInjection;
using ShaPrint.Core;

namespace ShaPrint.UI.ViewModels;

/// <summary>
/// Small DI helper shared by the page ViewModels.
///
/// Resolves optional / platform services without throwing when the service is registered but
/// cannot be constructed on the current platform. This happens on macOS/Linux before Task 7:
/// shared services whose constructors need a platform backend (e.g. <c>DiscoveryServerService</c>
/// needs <c>INotificationService</c> + <c>IPrinterManager</c> + <c>IScannerService</c>) are
/// registered by <c>AddSharedServices</c> but have no Unix implementation yet. Mirroring
/// WpfApp's null-tolerant <c>App.GetService&lt;T&gt;()</c> style (SettingsViewModel.ResetMode),
/// a null result degrades the affected feature gracefully instead of crashing the shell.
/// </summary>
internal static class ViewModelSupport
{
    public static T? Resolve<T>(IServiceProvider services) where T : class
    {
        try
        {
            return services.GetService<T>();
        }
        catch (Exception ex)
        {
            AppLogger.Error($"[DI] Could not resolve {typeof(T).Name}", ex);
            return null;
        }
    }
}