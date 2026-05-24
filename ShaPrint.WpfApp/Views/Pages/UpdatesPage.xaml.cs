using ShaPrint.WpfApp.ViewModels.Pages;
using System.Windows.Controls;
using Wpf.Ui.Abstractions.Controls;

namespace ShaPrint.WpfApp.Views.Pages
{
    public partial class UpdatesPage : INavigableView<UpdatesViewModel>
    {
        public UpdatesViewModel ViewModel { get; }

        public UpdatesPage(UpdatesViewModel viewModel)
        {
            ViewModel = viewModel;
            DataContext = this;
            InitializeComponent();
        }
    }
}
