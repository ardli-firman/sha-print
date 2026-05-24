using Microsoft.Extensions.DependencyInjection;
using Microsoft.Extensions.Hosting;
using ShaPrint.WpfApp.ViewModels.Pages;
using ShaPrint.WpfApp.ViewModels.Windows;
using ShaPrint.WpfApp.Views.Pages;
using ShaPrint.WpfApp.Views.Windows;
using ShaPrint.WpfApp.Services;
using System.Windows;
using Wpf.Ui;
using Wpf.Ui.Abstractions;

namespace ShaPrint.WpfApp
{
    public partial class App : Application
    {
        private static readonly IHost _host = Host
            .CreateDefaultBuilder()
            .ConfigureServices((context, services) =>
            {
                // WPF UI Services
                services.AddSingleton<INavigationViewPageProvider, NavigationViewPageProvider>();
                services.AddSingleton<INavigationService, NavigationService>();
                services.AddSingleton<ISnackbarService, SnackbarService>();
                services.AddSingleton<IContentDialogService, ContentDialogService>();

                // Windows
                services.AddSingleton<MainWindow>();
                services.AddSingleton<MainWindowViewModel>();

                // Pages
                services.AddTransient<WelcomePage>();
                services.AddTransient<WelcomeViewModel>();
                services.AddTransient<ServerPage>();
                services.AddSingleton<ServerViewModel>();
                services.AddTransient<ClientPage>();
                services.AddSingleton<ClientViewModel>();

                // Settings Page
                services.AddTransient<SettingsPage>();
                services.AddSingleton<SettingsViewModel>();

                // Background Services
                services.AddHostedService<UpdateService>();
            }).Build();

        public static T? GetService<T>() where T : class
        {
            return _host.Services.GetService(typeof(T)) as T;
        }

        private async void OnStartup(object sender, StartupEventArgs e)
        {
            this.DispatcherUnhandledException += (s, ex) =>
            {
                System.IO.File.AppendAllText("crash.log", $"[{System.DateTime.Now}] Dispatcher Exception: {ex.Exception}\n");
                ex.Handled = true; // prevent immediate close if possible, or just log
            };
            System.AppDomain.CurrentDomain.UnhandledException += (s, ex) =>
            {
                System.IO.File.AppendAllText("crash.log", $"[{System.DateTime.Now}] Unhandled Exception: {ex.ExceptionObject}\n");
            };

            await _host.StartAsync();

            bool isStartup = e.Args.Contains("--startup");

            var mainWindow = GetService<MainWindow>();
            
            // Auto-start engines in the background based on AppMode
            try
            {
                string dir = System.IO.Path.Combine(System.Environment.GetFolderPath(System.Environment.SpecialFolder.LocalApplicationData), "ShaPrint");
                string modeFile = System.IO.Path.Combine(dir, "AppMode.json");
                if (System.IO.File.Exists(modeFile))
                {
                    string json = System.IO.File.ReadAllText(modeFile);
                    string? mode = System.Text.Json.JsonSerializer.Deserialize<string>(json);
                    
                    if (mode == "Server")
                    {
                        GetService<ServerViewModel>(); // Initiates constructor and starts broadcasting
                    }
                    else if (mode == "Client")
                    {
                        GetService<ClientViewModel>(); // Initiates constructor and starts listening
                    }
                }
            }
            catch (System.Exception ex) 
            { 
                System.IO.File.AppendAllText("crash.log", $"[{System.DateTime.Now}] Background startup failed: {ex.Message}\n"); 
            }

            if (!isStartup)
            {
                mainWindow?.Show();
            }
        }

        private async void OnExit(object sender, ExitEventArgs e)
        {
            await _host.StopAsync();
            _host.Dispose();
        }
    }
}
