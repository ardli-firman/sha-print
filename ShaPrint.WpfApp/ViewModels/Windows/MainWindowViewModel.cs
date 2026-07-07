using CommunityToolkit.Mvvm.ComponentModel;
using CommunityToolkit.Mvvm.Input;
using System.Windows;

namespace ShaPrint.WpfApp.ViewModels.Windows
{
    public partial class MainWindowViewModel : ObservableObject
    {
        [ObservableProperty]
        private string _applicationTitle = "ShaPrint";

        [ObservableProperty]
        private Visibility _sidebarVisibility = Visibility.Collapsed;

        [ObservableProperty]
        private Visibility _welcomeVisibility = Visibility.Visible;

        [ObservableProperty]
        private Visibility _isServerMode = Visibility.Collapsed;

        [ObservableProperty]
        private Visibility _isClientMode = Visibility.Collapsed;

        [ObservableProperty]
        private Visibility _isMonitorMode = Visibility.Collapsed;

        public bool IsExiting { get; set; } = false;

        [RelayCommand]
        private void ExitApplication()
        {
            IsExiting = true;
            Application.Current.Shutdown();
        }
        
        [RelayCommand]
        private void ShowApplication()
        {
            var mainWindow = Application.Current.MainWindow;
            if (mainWindow != null)
            {
                mainWindow.Show();
                if (mainWindow.WindowState == WindowState.Minimized)
                {
                    mainWindow.WindowState = WindowState.Normal;
                }
                mainWindow.Activate();
            }
        }
    }
}
