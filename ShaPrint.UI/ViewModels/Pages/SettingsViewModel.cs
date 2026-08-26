using CommunityToolkit.Mvvm.ComponentModel;
using CommunityToolkit.Mvvm.Input;
using Microsoft.Extensions.DependencyInjection;
using ShaPrint.Core;
using ShaPrint.Platform.Abstractions;
using ShaPrint.UI.Models;
using System.Reflection;

namespace ShaPrint.UI.ViewModels.Pages;

/// <summary>
/// Settings page. Migrated from <c>ShaPrint.WpfApp/ViewModels/Pages/SettingsViewModel.cs</c>
/// (Task 5).
/// WPF -> Avalonia adaptations:
/// <list type="bullet">
/// <item><c>StartupManager</c> (static) -> <see cref="IStartupManager"/> (Windows adapter; no-op
/// gracefully before Task 7 lands a Unix startup manager).</item>
/// <item><c>App.GetService&lt;T&gt;()</c> -> <see cref="IServiceProvider"/> (ResetMode stops the
/// active engines).</item>
/// <item><c>OpenFolderDialog</c> (WPF) is deferred — see <see cref="BrowseFolder"/>.</item>
/// </list>
/// </summary>
public partial class SettingsViewModel : ObservableObject
{
    private readonly IStartupManager? _startupManager;
    private readonly IServiceProvider _serviceProvider;
    private readonly MainWindowViewModel _mainWindowViewModel;

    public SettingsViewModel(MainWindowViewModel mainWindowViewModel, IServiceProvider serviceProvider)
    {
        _mainWindowViewModel = mainWindowViewModel;
        _serviceProvider = serviceProvider;
        _startupManager = ViewModelSupport.Resolve<IStartupManager>(serviceProvider);

        AppVersionText = $"Version: {Assembly.GetExecutingAssembly().GetName().Version?.ToString() ?? "1.0.0.0"}";

        // Read settings
        var settings = AppSettings.Current;
        _autoUpdateEnabled = settings.AutoUpdateEnabled;
        _autoPurgeEnabled = settings.AutoPurgeEnabled;
        _channelIndex = settings.Channel == Services.UpdateChannel.Beta ? 1 : 0;
        _channelName = settings.NetworkChannel;
        _autoSaveScans = settings.AutoSaveScans;
        _defaultScansFolder = string.IsNullOrEmpty(settings.DefaultScansFolder)
            ? Path.Combine(Environment.GetFolderPath(Environment.SpecialFolder.MyDocuments), "ShaPrint Scans")
            : settings.DefaultScansFolder;

        if (settings.LastUpdateCheck > DateTime.MinValue)
        {
            _lastCheckedText = $"Last checked: {settings.LastUpdateCheck:g}";
        }

        EvaluateChannelStrength(_channelName);
    }

    [ObservableProperty]
    private bool _autoSaveScans;

    partial void OnAutoSaveScansChanged(bool value)
    {
        AppSettings.Current.AutoSaveScans = value;
        AppSettings.Save();
    }

    [ObservableProperty]
    private string _defaultScansFolder = string.Empty;

    partial void OnDefaultScansFolderChanged(string value)
    {
        AppSettings.Current.DefaultScansFolder = value;
        AppSettings.Save();
    }

    [RelayCommand]
    private void BrowseFolder()
    {
        // deferred: WPF OpenFolderDialog -> needs the Avalonia StorageProvider (view layer, Task 6).
        // The folder stays editable as text until then.
        StatusMessage = "Folder picker not available yet — type the path manually.";
    }

    [ObservableProperty]
    private string _statusMessage = "";

    public bool RunOnStartup
    {
        get => _startupManager?.IsStartupEnabled() ?? false;
        set
        {
            if (_startupManager == null)
            {
                StatusMessage = "Startup management not available on this platform yet.";
                return;
            }
            _startupManager.SetStartup(value);
            OnPropertyChanged(nameof(RunOnStartup));
        }
    }

    [ObservableProperty]
    private string _channelName;

    partial void OnChannelNameChanged(string value)
    {
        if (string.IsNullOrWhiteSpace(value)) return;

        if (value.Contains(" "))
        {
            ChannelName = value.Replace(" ", "");
            return;
        }

        EvaluateChannelStrength(value);

        AppSettings.Current.NetworkChannel = value;
        AppSettings.Save();
        ShaPrint.Core.Constants.SetNetworkChannel(value);
    }

    [ObservableProperty]
    private bool _isWeakChannel;

    private void EvaluateChannelStrength(string channel)
    {
        IsWeakChannel = channel == "DefaultChannel" || string.IsNullOrWhiteSpace(channel) || channel.Trim().Length < 8;
    }

    [ObservableProperty]
    private bool _autoUpdateEnabled;

    partial void OnAutoUpdateEnabledChanged(bool value)
    {
        AppSettings.Current.AutoUpdateEnabled = value;
        AppSettings.Save();
    }

    [ObservableProperty]
    private bool _autoPurgeEnabled;

    partial void OnAutoPurgeEnabledChanged(bool value)
    {
        AppSettings.Current.AutoPurgeEnabled = value;
        AppSettings.Save();
    }

    [ObservableProperty]
    private int _channelIndex;

    partial void OnChannelIndexChanged(int value)
    {
        AppSettings.Current.Channel = value == 1 ? Services.UpdateChannel.Beta : Services.UpdateChannel.Stable;
        AppSettings.Save();
    }

    [ObservableProperty]
    private string _lastCheckedText = "Last checked: Never";

    [ObservableProperty]
    private string _appVersionText = "Version: 1.0.0.0";

    [RelayCommand]
    private void ResetMode()
    {
        // deferred: bersihkan confirm dialog (WPF MessageBox) — view layer Task 6. Semantics kept.
        try
        {
            // Stop Server and Client if they are running
            var serverVM = _serviceProvider.GetService<ServerViewModel>();
            if (serverVM != null) serverVM.StopServer();

            var clientVM = _serviceProvider.GetService<ClientViewModel>();
            if (clientVM != null) clientVM.StopClient();
        }
        catch (Exception ex)
        {
            AppLogger.Error("[SETTINGS] Failed to stop active engines during mode reset", ex);
        }

        // Delete AppMode.json
        string modeFile = Path.Combine(Environment.GetFolderPath(Environment.SpecialFolder.LocalApplicationData), "ShaPrint", "AppMode.json");
        try
        {
            if (File.Exists(modeFile)) File.Delete(modeFile);
        }
        catch { }

        // Hide Sidebar again and show WelcomeFrame
        _mainWindowViewModel.ShowWelcome();
    }
}