using ShaPrint.WpfApp.ViewModels.Pages;
using System.Windows.Controls;

namespace ShaPrint.WpfApp.Views.Pages
{
    public partial class ServerPage : Page
    {
        public ServerViewModel ViewModel { get; }

        public ServerPage(ServerViewModel viewModel)
        {
            ViewModel = viewModel;
            DataContext = this;
            InitializeComponent();
            
            // Clean up resources when the page is unloaded if needed.
            // In a real app we might not want to dispose the view model if it's meant to stay alive in background.
            // Since ServerViewModel manages the server lifecycle, it should stay alive, so we won't dispose it here.
        }
    }
}
