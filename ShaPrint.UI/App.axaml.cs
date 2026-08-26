using Avalonia;
using Avalonia.Controls.ApplicationLifetimes;
using Avalonia.Markup.Xaml;
using Microsoft.Extensions.DependencyInjection;
using Microsoft.Extensions.Hosting;
using ShaPrint.UI.ViewModels;
using ShaPrint.UI.ViewModels.Pages;

namespace ShaPrint.UI;

public partial class App : Application
{
    /// <summary>
    /// DI host for the entire app. Built EXACTLY once, in
    /// <see cref="OnFrameworkInitializationCompleted"/> — never build a second host (the old
    /// plan's double-build mistake caused a race). Android (Task 9) wires its platform
    /// services via a static callback that runs BEFORE this callback, so a single host is
    /// enough for every platform.
    /// </summary>
    public static IHost Host { get; private set; } = null!;

    public override void Initialize()
    {
        AvaloniaXamlLoader.Load(this);
    }

    public override void OnFrameworkInitializationCompleted()
    {
        if (ApplicationLifetime is IClassicDesktopStyleApplicationLifetime desktop)
        {
            // Runtime platform switch lives in Program.ConfigureServices (OperatingSystem.*),
            // so the host is built the same way on every OS.
            var builder = new HostApplicationBuilder();
            Program.ConfigureServices(builder.Services);
            Host = builder.Build();

            // Task 5: shell DataContext comes from DI. Views/Pages/*.axaml (Task 6) bind their
            // DataContext off MainWindowViewModel.CurrentPage via DataTemplates.
            desktop.MainWindow = new Views.MainWindow
            {
                DataContext = Host.Services.GetRequiredService<MainWindowViewModel>()
            };

            // Start hosted services (UpdateService background auto-check) — WpfApp awaited
            // _host.StartAsync() in OnStartup. Fire-and-forget: HostedService start is async.
            _ = Host.StartAsync();

            // Restore the persisted mode (AppMode.json) — WelcomeViewModel drives the shell
            // navigation (replaces the WPF WelcomePage code-behind call).
            try
            {
                Host.Services.GetRequiredService<WelcomeViewModel>().CheckSavedModeAndNavigate();
            }
            catch (Exception ex)
            {
                System.Diagnostics.Debug.WriteLine($"Mode restore failed: {ex.Message}");
            }
        }

        base.OnFrameworkInitializationCompleted();
    }
}