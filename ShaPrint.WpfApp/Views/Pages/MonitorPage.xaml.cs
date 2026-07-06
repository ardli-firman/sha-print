using System;
using System.Windows;
using System.Windows.Controls;
using ShaPrint.WpfApp.Services.Monitor;
using ShaPrint.WpfApp.ViewModels.Pages;

namespace ShaPrint.WpfApp.Views.Pages
{
    public partial class MonitorPage : Page
    {
        private readonly MonitorService _monitorService;
        public MonitorViewModel ViewModel { get; }

        public MonitorPage(MonitorViewModel viewModel, MonitorService monitorService)
        {
            ViewModel = viewModel;
            _monitorService = monitorService;
            DataContext = this;
            InitializeComponent();
        }

        private async void RefreshButton_Click(object sender, RoutedEventArgs e)
        {
            if (RefreshButton != null)
            {
                RefreshButton.IsEnabled = false;
            }
            try
            {
                await _monitorService.TriggerManualRefreshAsync();
            }
            finally
            {
                if (RefreshButton != null)
                {
                    RefreshButton.IsEnabled = true;
                }
            }
        }
    }
}
