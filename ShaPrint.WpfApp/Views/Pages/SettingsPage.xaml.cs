using System.Windows.Controls;
using ShaPrint.WpfApp.ViewModels.Pages;
using Wpf.Ui.Abstractions.Controls;

namespace ShaPrint.WpfApp.Views.Pages
{
    public partial class SettingsPage : INavigableView<SettingsViewModel>
    {
        public SettingsViewModel ViewModel { get; }

        public SettingsPage(SettingsViewModel viewModel)
        {
            ViewModel = viewModel;
            DataContext = this;
            InitializeComponent();
        }
    }
}
