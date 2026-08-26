using CommunityToolkit.Mvvm.ComponentModel;
using CommunityToolkit.Mvvm.Input;
using ShaPrint.UI.Models;
using ShaPrint.UI.Services;
using System.Collections.ObjectModel;

namespace ShaPrint.UI.ViewModels.Pages;

/// <summary>
/// Updates page. Migrated from <c>ShaPrint.WpfApp/ViewModels/Pages/UpdatesViewModel.cs</c>
/// (Task 5) onto the shared <see cref="Services.UpdateService"/> (Task 4, IHostedService).
///
/// Real paths: <see cref="LoadReleasesAsync"/> calls <see cref="UpdateService.GetAvailableReleasesAsync"/>
/// (GitHub), channel switching persists via <see cref="AppSettings"/>, and
/// <see cref="InstallSelected"/> launches the updater via <see cref="UpdateService.LaunchUpdater"/>.
///
/// WPF -> Avalonia adaptations: the <c>MessageBox</c> confirmation is a view concern (deferred);
/// <c>UpdateChannel</c> comes from <see cref="Services.UpdateChannel"/> (UI copy, same values).
/// </summary>
public partial class UpdatesViewModel : ObservableObject
{
    private readonly UpdateService _updateService;

    public UpdatesViewModel(UpdateService updateService)
    {
        _updateService = updateService;
        SelectedChannel = AppSettings.Current.Channel == UpdateChannel.Beta ? "Beta" : "Stable";
        LoadReleasesCommand = new AsyncRelayCommand(LoadReleasesAsync);
        InstallSelectedCommand = new RelayCommand(InstallSelected, () => SelectedRelease != null);

        // The background auto-check (UpdateService as IHostedService) notifies here instead of
        // a WPF MessageBox.
        _updateService.UpdateAvailable += OnUpdateAvailable;
    }

    private void OnUpdateAvailable(object? sender, UpdateAvailableEventArgs e)
    {
        StatusMessage = $"Update available: {e.Release.Name} ({e.Release.Version}) — open this page to install.";
    }

    [ObservableProperty]
    private ObservableCollection<GitHubRelease> releases = new();

    [ObservableProperty]
    [NotifyCanExecuteChangedFor(nameof(InstallSelectedCommand))]
    private GitHubRelease? selectedRelease;

    [ObservableProperty]
    private string statusMessage = "Ready";

    [ObservableProperty]
    private string selectedChannel;

    public string[] AvailableChannels { get; } = { "Stable", "Beta" };

    partial void OnSelectedChannelChanged(string value)
    {
        var channel = value == "Beta" ? UpdateChannel.Beta : UpdateChannel.Stable;
        if (AppSettings.Current.Channel != channel)
        {
            AppSettings.Current.Channel = channel;
            AppSettings.Save();
            StatusMessage = $"Auto-update channel changed to {channel}.";
            LoadReleasesCommand.Execute(null);
        }
    }

    public IAsyncRelayCommand LoadReleasesCommand { get; }
    public IRelayCommand InstallSelectedCommand { get; }

    public async Task OnNavigatedToAsync()
    {
        if (Releases.Count == 0)
        {
            await LoadReleasesAsync();
        }
    }

    private async Task LoadReleasesAsync()
    {
        StatusMessage = "Loading releases from GitHub...";
        Releases.Clear();

        var loadedReleases = await _updateService.GetAvailableReleasesAsync();

        if (loadedReleases.Count == 0)
        {
            StatusMessage = "No releases found or network error.";
            return;
        }

        Version currentVersion = System.Reflection.Assembly.GetExecutingAssembly().GetName().Version ?? new Version(0, 0, 0, 0);

        var filteredReleases = loadedReleases.Where(r => r.Channel == AppSettings.Current.Channel).ToList();

        foreach (var release in filteredReleases.OrderByDescending(r => r.Version))
        {
            Releases.Add(release);
        }

        StatusMessage = $"Found {filteredReleases.Count} releases. Current version: {currentVersion}";
    }

    private void InstallSelected()
    {
        if (SelectedRelease == null) return;

        Version currentVersion = System.Reflection.Assembly.GetExecutingAssembly().GetName().Version ?? new Version(0, 0, 0, 0);

        string action = SelectedRelease.Version > currentVersion ? "upgrade" :
                        SelectedRelease.Version < currentVersion ? "downgrade" : "reinstall";

        // deferred: WPF MessageBox confirm (YesNo) -> view-layer confirmation (Task 6).
        StatusMessage = $"Launching updater for {action} to {SelectedRelease.Version} ({SelectedRelease.Channel})...";
        _updateService.LaunchUpdater(SelectedRelease.DownloadUrl);
    }
}