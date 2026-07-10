using CommunityToolkit.Mvvm.ComponentModel;
using CommunityToolkit.Mvvm.Input;
using ShaPrint.WpfApp.Models;
using System;
using System.IO;
using System.Reflection;
using Wpf.Ui;
using ShaPrint.WpfApp.Views.Pages;

namespace ShaPrint.WpfApp.ViewModels.Pages
{
    public partial class SettingsViewModel : ObservableObject
    {
        private readonly INavigationService _navigationService;
        private readonly Windows.MainWindowViewModel _mainWindowViewModel;

        public SettingsViewModel(INavigationService navigationService, Windows.MainWindowViewModel mainWindowViewModel)
        {
            _navigationService = navigationService;
            _mainWindowViewModel = mainWindowViewModel;

            AppVersionText = $"Version: {Assembly.GetExecutingAssembly().GetName().Version?.ToString() ?? "1.0.0.0"}";
            
            // Read settings
            var settings = AppSettings.Current;
            _autoUpdateEnabled = settings.AutoUpdateEnabled;
            _autoPurgeEnabled = settings.AutoPurgeEnabled;
            _channelIndex = settings.Channel == UpdateChannel.Beta ? 1 : 0;
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
            var dialog = new Microsoft.Win32.OpenFolderDialog
            {
                Title = "Select Default Folder to Save Scans",
                InitialDirectory = string.IsNullOrEmpty(DefaultScansFolder)
                    ? Environment.GetFolderPath(Environment.SpecialFolder.MyDocuments)
                    : DefaultScansFolder
            };
 
            if (dialog.ShowDialog() == true)
            {
                DefaultScansFolder = dialog.FolderName;
            }
        }

        public bool RunOnStartup
        {
            get => Utils.StartupManager.IsStartupEnabled();
            set
            {
                Utils.StartupManager.SetStartup(value);
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
            AppSettings.Current.Channel = value == 1 ? UpdateChannel.Beta : UpdateChannel.Stable;
            AppSettings.Save();
        }

        [ObservableProperty]
        private string _lastCheckedText = "Last checked: Never";

        [ObservableProperty]
        private string _appVersionText = "Version: 1.0.0.0";

        [RelayCommand]
        private void ResetMode()
        {
            var result = System.Windows.MessageBox.Show(
                "Are you sure you want to switch mode?\n\nThis will stop any active server/client connections and reset the application mode.", 
                "Switch Mode", 
                System.Windows.MessageBoxButton.YesNo, 
                System.Windows.MessageBoxImage.Question);
            
            if (result == System.Windows.MessageBoxResult.Yes)
            {
                // Stop Server and Client if they are running
                var serverVM = App.GetService<ServerViewModel>();
                if (serverVM != null) serverVM.StopServer();

                var clientVM = App.GetService<ClientViewModel>();
                if (clientVM != null) clientVM.StopClient();

                // Delete AppMode.json
                string modeFile = Path.Combine(Environment.GetFolderPath(Environment.SpecialFolder.LocalApplicationData), "ShaPrint", "AppMode.json");
                try
                {
                    if (File.Exists(modeFile)) File.Delete(modeFile);
                }
                catch { }

                // Hide Sidebar again and show WelcomeFrame
                _mainWindowViewModel.SidebarVisibility = System.Windows.Visibility.Collapsed;
                _mainWindowViewModel.WelcomeVisibility = System.Windows.Visibility.Visible;

                // We don't need to navigate the RootNavigation because WelcomeFrame covers it!
                // But just in case, we can clear the navigation or navigate to an empty page if needed.
                // _navigationService.Navigate(typeof(WelcomePage)); 
                // We actually don't navigate to WelcomePage using NavigationService anymore, because WelcomePage is in WelcomeFrame.
            }
        }
    }
}
