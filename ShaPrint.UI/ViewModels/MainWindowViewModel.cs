using Avalonia;
using Avalonia.Controls;
using Avalonia.Controls.ApplicationLifetimes;
using CommunityToolkit.Mvvm.ComponentModel;
using CommunityToolkit.Mvvm.Input;
using ShaPrint.UI.ViewModels.Pages;

namespace ShaPrint.UI.ViewModels;

/// <summary>
/// Window shell + navigation surface. Migrated from
/// <c>ShaPrint.WpfApp/ViewModels/Windows/MainWindowViewModel.cs</c> (Task 5).
///
/// WPF -> Avalonia adaptations:
/// <list type="bullet">
/// <item>WPF-UI <c>INavigationService</c> is replaced by <see cref="CurrentPage"/> hosted in a
/// <c>ContentControl</c> (see Views/MainWindow.axaml, Task 3/6). WelcomeViewModel and
/// SettingsViewModel switch pages through <see cref="NavigateToServer"/> /
/// <see cref="NavigateToClient"/> / <see cref="NavigateToMonitor"/> / <see cref="ShowWelcome"/>.</item>
/// <item>WPF <c>Visibility</c> tri-state props collapse to bools (<see cref="SidebarVisible"/>,
/// <see cref="WelcomeVisible"/>, mode flags).</item>
/// <item>Tray/NotifyIcon "show application" is exposed as <see cref="ShowRequested"/> plus a
/// to-front behaviour implemented with the Avalonia lifetime (tray icon wiring is a Task 6
/// concern — the command stays callable).</item>
/// </list>
/// </summary>
public partial class MainWindowViewModel : ObservableObject
{
    [ObservableProperty]
    private string _applicationTitle = "ShaPrint";

    /// <summary>Sidebar hidden on the Welcome page (was WPF Visibility.Collapsed).</summary>
    [ObservableProperty]
    private bool _sidebarVisible;

    [ObservableProperty]
    private bool _welcomeVisible = true;

    [ObservableProperty]
    private bool _isServerMode;

    [ObservableProperty]
    private bool _isClientMode;

    [ObservableProperty]
    private bool _isMonitorMode;

    /// <summary>Page ViewModel currently hosted by the shell's ContentControl.</summary>
    [ObservableProperty]
    private ObservableObject? _currentPage;

    public bool IsExiting { get; set; } = false;

    /// <summary>Raised when the app should be brought to the foreground (tray/notification activation).</summary>
    public event EventHandler? ShowRequested;

    [RelayCommand]
    private void ExitApplication()
    {
        IsExiting = true;
        // Avalonia Application has no Shutdown(); the desktop lifetime owns it.
        (Application.Current?.ApplicationLifetime as IClassicDesktopStyleApplicationLifetime)?.Shutdown();
    }

    [RelayCommand]
    private void ShowApplication()
    {
        ShowRequested?.Invoke(this, EventArgs.Empty);

        // Mirror WpfApp's OnActivateRequested: bring the main window to the front.
        var lifetime = Application.Current?.ApplicationLifetime as IClassicDesktopStyleApplicationLifetime;
        var window = lifetime?.MainWindow;
        if (window == null) return;

        window.Show();
        if (window.WindowState == WindowState.Minimized)
        {
            window.WindowState = WindowState.Normal;
        }
        window.Activate();
        window.Topmost = true;
        window.Topmost = false;
        window.Focus();
    }

    // ── Navigation surface (replaces WPF-UI INavigationService) ──────────────

    /// <summary>Back to the Welcome page: hide sidebar, drop the current page.</summary>
    public void ShowWelcome()
    {
        WelcomeVisible = true;
        SidebarVisible = false;
        IsServerMode = false;
        IsClientMode = false;
        IsMonitorMode = false;
        CurrentPage = null;
    }

    public void NavigateToServer(ServerViewModel page) => NavigateToMode(page, isServerMode: true);

    public void NavigateToClient(ClientViewModel page) => NavigateToMode(page, isClientMode: true);

    public void NavigateToMonitor(MonitorViewModel page) => NavigateToMode(page, isMonitorMode: true);

    private void NavigateToMode(ObservableObject page, bool isServerMode = false, bool isClientMode = false, bool isMonitorMode = false)
    {
        IsServerMode = isServerMode;
        IsClientMode = isClientMode;
        IsMonitorMode = isMonitorMode;
        WelcomeVisible = false;
        SidebarVisible = true;
        CurrentPage = page;
    }
}