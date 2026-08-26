using Avalonia;
using Microsoft.Extensions.DependencyInjection;
using ShaPrint.UI.Cli;

namespace ShaPrint.UI;

public static class Program
{
    // Initialization code. Don't use any Avalonia, third-party APIs or any
    // SynchronizationContext-reliant code before AppMain is called: things aren't initialized
    // yet and stuff might break.
    [STAThread]
    public static void Main(string[] args)
    {
        // CLI branch (`shaprint send --printer <name> --file <path> [--host <ip>]`) is handled
        // before any Avalonia/GUI startup. Task 8 fills CliDispatcher in; for now it always
        // returns false so the app always launches the desktop shell.
        if (CliDispatcher.TryHandle(args))
            return;

        BuildAvaloniaApp().StartWithClassicDesktopLifetime(args);
    }

    public static AppBuilder BuildAvaloniaApp() =>
        AppBuilder.Configure<App>()
            .UsePlatformDetect()
            .WithInterFont()
            .LogToTrace();

    /// <summary>
    /// Runtime DI switch: picks the platform backend by the OS the process runs on, not by
    /// the TFM it was compiled for. Windows -> ShaPrint.Platform.Windows adapters;
    /// macOS/Linux -> Unix stubs (Task 7 fills them in); Android -> wired via static
    /// callback from MainActivity (Task 9).
    /// </summary>
    public static void ConfigureServices(IServiceCollection services)
    {
        if (OperatingSystem.IsWindows())
        {
            services.AddPlatformWindows();
        }
        else if (OperatingSystem.IsMacOS() || OperatingSystem.IsLinux())
        {
            services.AddPlatformUnix();
        }
    }
}