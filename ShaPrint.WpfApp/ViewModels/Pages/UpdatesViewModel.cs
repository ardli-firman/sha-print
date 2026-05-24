using CommunityToolkit.Mvvm.ComponentModel;
using CommunityToolkit.Mvvm.Input;
using ShaPrint.Core;
using ShaPrint.WpfApp.Models;
using ShaPrint.WpfApp.Services;
using System;
using System.Collections.ObjectModel;
using System.Linq;
using System.Threading.Tasks;
using System.Windows;

namespace ShaPrint.WpfApp.ViewModels.Pages
{
    public partial class UpdatesViewModel : ObservableObject
    {
        private readonly UpdateService _updateService;

        public UpdatesViewModel(UpdateService updateService)
        {
            _updateService = updateService;
            SelectedChannel = AppSettings.Current.Channel == UpdateChannel.Beta ? "Beta" : "Stable";
            LoadReleasesCommand = new AsyncRelayCommand(LoadReleasesAsync);
            InstallSelectedCommand = new RelayCommand(InstallSelected, () => SelectedRelease != null);
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

            var result = MessageBox.Show($"Are you sure you want to {action} to {SelectedRelease.Version} ({SelectedRelease.Channel})?",
                "Confirm Installation", MessageBoxButton.YesNo, MessageBoxImage.Question);

            if (result == MessageBoxResult.Yes)
            {
                _updateService.LaunchUpdaterAndExit(SelectedRelease.DownloadUrl);
            }
        }
    }
}
