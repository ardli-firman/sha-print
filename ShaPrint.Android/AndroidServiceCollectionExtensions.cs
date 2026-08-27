using Android.Content;
using Microsoft.Extensions.DependencyInjection;
using ShaPrint.Android.Services;
using ShaPrint.Android.ViewModels;
using ShaPrint.Platform.Abstractions;

namespace ShaPrint.Android;

/// <summary>
/// Android platform DI registrations (Task 9). Wired into the single Host via the static
/// callback set by <see cref="MainActivity"/> — see <see cref="App.PlatformServiceRegistration"/>.
/// </summary>
public static class AndroidServiceCollectionExtensions
{
    public static IServiceCollection AddAndroidServices(this IServiceCollection services)
    {
        // Application context (not an Activity): safe for services that outlive any single
        // activity instance (rotation/recreation). Resolved via the static Android
        // Application.Context (global:: — namespace ShaPrint.Android shadows `Android`) so no
        // Activity instance leaks into the container.
        services.AddSingleton(_ => global::Android.App.Application.Context);

        // UDP discovery + print relay (the relay is also the IPrintRelayClient used by the
        // shared orchestration path; no OS-agnostic relay exists for Android to wrap).
        services.AddSingleton<DiscoveryService>();
        services.AddSingleton<PrintRelayService>();
        services.AddSingleton<IPrintRelayClient>(sp => sp.GetRequiredService<PrintRelayService>());

        services.AddSingleton<MainViewModel>();

        return services;
    }
}
