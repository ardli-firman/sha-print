using Avalonia.Controls;
using Avalonia.Interactivity;
using Microsoft.Extensions.DependencyInjection;
using ShaPrint.UI.ViewModels;
using ShaPrint.UI.ViewModels.Pages;

namespace ShaPrint.UI.Views;

public partial class MainWindow : Window
{
    public MainWindow()
    {
        InitializeComponent();

        // WelcomeViewModel is DI-transient and takes IServiceProvider (no parameterless ctor),
        // so it cannot be a Design.DataContext and is not a property on the shell VM. Resolve it
        // once here; App.Host is built before the window is created (App.OnFrameworkInitializationCompleted).
        WelcomeHost.DataContext = App.Host.Services.GetRequiredService<WelcomeViewModel>();
    }

    // ── Sidebar navigation ─────────────────────────────────────────────────────
    // The shell VM is the navigation surface (CurrentPage setter is private) but it cannot
    // resolve page VMs from DI itself, so the buttons resolve the page VM and hand it over —
    // the same pattern WelcomeViewModel.NavigateToMode uses for the Welcome mode buttons.

    private void OnNavigateServer(object? sender, RoutedEventArgs e)
    {
        if (DataContext is MainWindowViewModel vm)
        {
            vm.NavigateToServer(App.Host.Services.GetRequiredService<ServerViewModel>());
        }
    }

    private void OnNavigateClient(object? sender, RoutedEventArgs e)
    {
        if (DataContext is MainWindowViewModel vm)
        {
            vm.NavigateToClient(App.Host.Services.GetRequiredService<ClientViewModel>());
        }
    }

    private void OnNavigateMonitor(object? sender, RoutedEventArgs e)
    {
        if (DataContext is MainWindowViewModel vm)
        {
            var monitor = App.Host.Services.GetRequiredService<MonitorViewModel>();
            monitor.Start(); // Start the polling service — same as WelcomeViewModel.NavigateToMode.
            vm.NavigateToMonitor(monitor);
        }
    }

    private void OnNavigateSettings(object? sender, RoutedEventArgs e)
    {
        if (DataContext is MainWindowViewModel vm)
        {
            vm.NavigateToSettings(App.Host.Services.GetRequiredService<SettingsViewModel>());
        }
    }

    private void OnNavigateUpdates(object? sender, RoutedEventArgs e)
    {
        if (DataContext is MainWindowViewModel vm)
        {
            var updates = App.Host.Services.GetRequiredService<UpdatesViewModel>();
            vm.NavigateToUpdates(updates);
            // Mirror the WPF page's OnNavigatedTo: load releases when entering the page.
            _ = updates.OnNavigatedToAsync();
        }
    }
}
