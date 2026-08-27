using Avalonia;
using Avalonia.Controls.ApplicationLifetimes;
using Avalonia.Markup.Xaml;
using Microsoft.Extensions.DependencyInjection;
using Microsoft.Extensions.Hosting;
using ShaPrint.Android.ViewModels;

namespace ShaPrint.Android;

/// <summary>
/// Android Application class (Task 9, choice (b) — see App.axaml for why ShaPrint.UI.App is
/// not reused). Hosts the DI container exactly like the desktop App (ShaPrint.UI.App), built
/// EXACTLY once in <see cref="OnFrameworkInitializationCompleted"/>.
/// </summary>
// Fully-qualified Avalonia.Application: inside namespace ShaPrint.Android the plain name
// `Application` is ambiguous with Android.App.Application (namespace shadowing).
public partial class App : Avalonia.Application
{
    /// <summary>
    /// DI host for the whole Android app. Built EXACTLY once, in
    /// <see cref="OnFrameworkInitializationCompleted"/> — never build a second host (the
    /// desktop plan's double-build mistake caused a race; this pattern prevents it).
    /// </summary>
    public static IHost Host { get; private set; } = null!;

    /// <summary>
    /// Static callback the platform services are registered through. <see cref="MainActivity"/>
    /// assigns it inside <see cref="MainActivity.CustomizeAppBuilder(AppBuilder)"/> — which the
    /// Avalonia.Android runtime invokes BEFORE <see cref="OnFrameworkInitializationCompleted"/>
    /// (see <c>AvaloniaMainActivity.InitializeAvaloniaView</c>: <c>CreateAppBuilder()</c> →
    /// <c>CustomizeAppBuilder()</c> → <c>SetupWithLifetime()</c> → this callback) — so the
    /// single host below always sees the Android registrations: no race, no second host.
    ///
    /// <para>Default value = the Android registrations, so a null/invalid registration can
    /// never occur even if an Activity never assigns it (defensive default, not an
    /// alternate host path).</para>
    /// </summary>
    public static Action<IServiceCollection> PlatformServiceRegistration { get; set; } =
        static services => services.AddAndroidServices();

    public override void Initialize()
    {
        AvaloniaXamlLoader.Load(this);
    }

    public override void OnFrameworkInitializationCompleted()
    {
        if (ApplicationLifetime is ISingleViewApplicationLifetime singleView)
        {
            // Runtime platform registration lives behind the static callback set by
            // MainActivity (see class doc above), so the host is built the same way on every
            // launch and only once.
            var builder = new HostApplicationBuilder();
            PlatformServiceRegistration(builder.Services);
            Host = builder.Build();

            singleView.MainView = new Views.MainView
            {
                DataContext = Host.Services.GetRequiredService<MainViewModel>()
            };

            // Start hosted services, mirroring the desktop App (fire-and-forget: hosted start
            // is async). Android registers none today; keeping the call means future Android
            // hosted services behave identically to desktop.
            _ = Host.StartAsync();
        }

        base.OnFrameworkInitializationCompleted();
    }
}
