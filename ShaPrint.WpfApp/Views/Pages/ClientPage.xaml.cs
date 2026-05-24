using ShaPrint.WpfApp.ViewModels.Pages;
using System.Windows.Controls;

namespace ShaPrint.WpfApp.Views.Pages
{
    public partial class ClientPage : Page
    {
        public ClientViewModel ViewModel { get; }

        public ClientPage(ClientViewModel viewModel)
        {
            ViewModel = viewModel;
            DataContext = this;
            InitializeComponent();
        }
    }
}
