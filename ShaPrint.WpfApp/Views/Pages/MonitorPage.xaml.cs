using System;
using System.Windows;
using System.Windows.Controls;
using ShaPrint.WpfApp.Services.Monitor;
using ShaPrint.WpfApp.ViewModels.Pages;

namespace ShaPrint.WpfApp.Views.Pages
{
    public partial class MonitorPage : Page
    {
        public MonitorViewModel ViewModel { get; }

        public MonitorPage(MonitorViewModel viewModel)
        {
            ViewModel = viewModel;
            DataContext = this;
            InitializeComponent();
        }
    }
}
