using CommunityToolkit.Mvvm.ComponentModel;
using CommunityToolkit.Mvvm.Input;
using System;
using System.IO;
using System.Text.Json;
using Wpf.Ui;

namespace ShaPrint.WpfApp.ViewModels.Pages
{
    public partial class WelcomeViewModel : ObservableObject
    {
        private readonly INavigationService _navigationService;
        private readonly Windows.MainWindowViewModel _mainWindowViewModel;
        private readonly string _modeFile;

        public WelcomeViewModel(INavigationService navigationService, Windows.MainWindowViewModel mainWindowViewModel)
        {
            _navigationService = navigationService;
            _mainWindowViewModel = mainWindowViewModel;
            
            string dir = Path.Combine(Environment.GetFolderPath(Environment.SpecialFolder.LocalApplicationData), "ShaPrint");
            if (!Directory.Exists(dir)) Directory.CreateDirectory(dir);
            _modeFile = Path.Combine(dir, "AppMode.json");
            
            // Ensure sidebar is hidden on welcome page
            _mainWindowViewModel.WelcomeVisibility = System.Windows.Visibility.Visible;
            _mainWindowViewModel.SidebarVisibility = System.Windows.Visibility.Collapsed;
        }

        public void CheckSavedModeAndNavigate()
        {
            if (File.Exists(_modeFile))
            {
                try
                {
                    string json = File.ReadAllText(_modeFile);
                    string? mode = JsonSerializer.Deserialize<string>(json);
                    
                    if (mode == "Server")
                    {
                        SelectServerMode();
                        return;
                    }
                    else if (mode == "Client")
                    {
                        SelectClientMode();
                        return;
                    }
                }
                catch { }
            }
        }

        [RelayCommand]
        private void SelectServerMode()
        {
            SaveMode("Server");
            _mainWindowViewModel.IsServerMode = System.Windows.Visibility.Visible;
            _mainWindowViewModel.IsClientMode = System.Windows.Visibility.Collapsed;
            _mainWindowViewModel.WelcomeVisibility = System.Windows.Visibility.Collapsed;
            _mainWindowViewModel.SidebarVisibility = System.Windows.Visibility.Visible;
            
            System.Windows.Application.Current.Dispatcher.InvokeAsync(() => 
                _navigationService.Navigate(typeof(Views.Pages.ServerPage)));
        }

        [RelayCommand]
        private void SelectClientMode()
        {
            SaveMode("Client");
            _mainWindowViewModel.IsServerMode = System.Windows.Visibility.Collapsed;
            _mainWindowViewModel.IsClientMode = System.Windows.Visibility.Visible;
            _mainWindowViewModel.WelcomeVisibility = System.Windows.Visibility.Collapsed;
            _mainWindowViewModel.SidebarVisibility = System.Windows.Visibility.Visible;
            
            System.Windows.Application.Current.Dispatcher.InvokeAsync(() => 
                _navigationService.Navigate(typeof(Views.Pages.ClientPage)));
        }

        private void SaveMode(string mode)
        {
            try
            {
                File.WriteAllText(_modeFile, JsonSerializer.Serialize(mode));
            }
            catch { }
        }
    }
}
