using ShaPrint.WpfApp.ViewModels.Pages;
using System.Windows.Controls;

namespace ShaPrint.WpfApp.Views.Pages
{
    public partial class WelcomePage : Page
    {
        public WelcomeViewModel ViewModel { get; }

        public WelcomePage(WelcomeViewModel viewModel)
        {
            ViewModel = viewModel;
            DataContext = this;
            InitializeComponent();
            
            this.Loaded += (s, e) => ViewModel.CheckSavedModeAndNavigate();
        }
    }
}
