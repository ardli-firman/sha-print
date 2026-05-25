using ShaPrint.WpfApp.ViewModels.Windows;
using System.Windows;
using Wpf.Ui;
using Wpf.Ui.Controls;

namespace ShaPrint.WpfApp.Views.Windows
{
    public partial class MainWindow : FluentWindow
    {
        public MainWindowViewModel ViewModel { get; }

        public MainWindow(MainWindowViewModel viewModel, INavigationService navigationService, ISnackbarService snackbarService, Pages.WelcomePage welcomePage)
        {
            ViewModel = viewModel;
            DataContext = this;
            InitializeComponent();
            
            navigationService.SetNavigationControl(RootNavigation);
            snackbarService.SetSnackbarPresenter(SnackbarPresenter);

            // Set WelcomeFrame content
            WelcomeFrame.Content = welcomePage;
        }

        protected override void OnClosing(System.ComponentModel.CancelEventArgs e)
        {
            // Intercept closing to hide instead of exit, creating the system tray stealth mode
            e.Cancel = true;
            this.Hide();
            
            TrayIcon.ShowBalloonTip("ShaPrint", "The application is hidden in the System Tray and running in the background.", Hardcodet.Wpf.TaskbarNotification.BalloonIcon.Info);
            
            base.OnClosing(e);
        }
    }
}
