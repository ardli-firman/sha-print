using Avalonia;
using Avalonia.Controls.ApplicationLifetimes;
using Avalonia.Markup.Xaml;
using Microsoft.Extensions.DependencyInjection;
using Microsoft.Extensions.Hosting;

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

            // Plain window for now; Task 5 wires MainWindowViewModel:
            //   desktop.MainWindow = new MainWindow { DataContext = Host.Services.GetRequiredService<MainWindowViewModel>() };
            desktop.MainWindow = new Views.MainWindow();
        }

        base.OnFrameworkInitializationCompleted();
    }
}