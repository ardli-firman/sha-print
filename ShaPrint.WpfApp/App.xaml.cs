using Microsoft.Extensions.DependencyInjection;
using Microsoft.Extensions.Hosting;
using ShaPrint.WpfApp.Helpers;
using ShaPrint.WpfApp.ViewModels.Pages;
using ShaPrint.WpfApp.ViewModels.Windows;
using ShaPrint.WpfApp.Views.Pages;
using ShaPrint.WpfApp.Views.Windows;
using ShaPrint.WpfApp.Services;
using System.IO;
using System.Windows;
using Wpf.Ui;
using Wpf.Ui.Abstractions;

namespace ShaPrint.WpfApp
{
    public partial class App : Application
    {
        private static SingleInstanceEnforcer? _singleInstanceEnforcer;

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

                // Updates Page
                services.AddTransient<UpdatesPage>();
                services.AddTransient<UpdatesViewModel>();

                // Background Services
                services.AddSingleton<UpdateService>();
                services.AddHostedService(provider => provider.GetRequiredService<UpdateService>());
                services.AddSingleton<ShaPrint.WpfApp.Services.Server.PrintMonitorService>();
            }).Build();

        public static T? GetService<T>() where T : class
        {
            return _host.Services.GetService(typeof(T)) as T;
        }

        private async void OnStartup(object sender, StartupEventArgs e)
        {
            // ── Single-instance enforcement ──────────────────────────────
            _singleInstanceEnforcer = new SingleInstanceEnforcer(OnActivateRequested);
            if (!_singleInstanceEnforcer.TryAcquire())
            {
                // Another instance is already running — it was signalled to
                // bring its window to front. Exit this instance immediately.
                _singleInstanceEnforcer.Dispose();
                _singleInstanceEnforcer = null;
                Shutdown();
                return;
            }

            // ── Original startup logic ───────────────────────────────────
            this.DispatcherUnhandledException += (s, ex) =>
            {
                File.AppendAllText("crash.log", $"[{System.DateTime.Now}] Dispatcher Exception: {ex.Exception}\n");
                ex.Handled = true;
            };
            System.AppDomain.CurrentDomain.UnhandledException += (s, ex) =>
            {
                File.AppendAllText("crash.log", $"[{System.DateTime.Now}] Unhandled Exception: {ex.ExceptionObject}\n");
            };

            await _host.StartAsync();

            bool isStartup = e.Args.Contains("--startup");

            var mainWindow = GetService<MainWindow>();

            // Inject dynamic network channel from AppSettings to Core
            ShaPrint.Core.Constants.SetNetworkChannel(ShaPrint.WpfApp.Models.AppSettings.Current.NetworkChannel);

            // Auto-start engines in the background based on AppMode
            try
            {
                string dir = Path.Combine(Environment.GetFolderPath(Environment.SpecialFolder.LocalApplicationData), "ShaPrint");
                string modeFile = Path.Combine(dir, "AppMode.json");
                if (File.Exists(modeFile))
                {
                    string json = File.ReadAllText(modeFile);
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
            catch (Exception ex)
            {
                File.AppendAllText("crash.log", $"[{System.DateTime.Now}] Background startup failed: {ex.Message}\n");
            }

            if (!isStartup)
            {
                mainWindow?.Show();
            }
        }

        private async void OnExit(object sender, ExitEventArgs e)
        {
            _singleInstanceEnforcer?.Dispose();
            _singleInstanceEnforcer = null;

            await _host.StopAsync();
            _host.Dispose();
        }

        /// <summary>
        /// Called on the listener thread when a second-instance activation
        /// signal is received. Brings the main window to the foreground.
        /// </summary>
        private void OnActivateRequested()
        {
            Dispatcher.Invoke(() =>
            {
                var window = Current?.MainWindow;
                if (window == null) return;

                // If window was hidden to tray, show it first
                window.Show();

                if (window.WindowState == WindowState.Minimized)
                    window.WindowState = WindowState.Normal;

                window.Activate();
                window.Topmost = true;
                window.Topmost = false;
                window.Focus();
            });
        }
    }
}
