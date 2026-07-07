using System;
using System.Windows;
using System.Windows.Controls;
using System.Windows.Input;
using ShaPrint.WpfApp.Services.Monitor;
using ShaPrint.WpfApp.ViewModels.Pages;

namespace ShaPrint.WpfApp.Views.Pages
{
    public partial class MonitorPage : Page
    {
        public MonitorViewModel ViewModel { get; }

        public MonitorPage(MonitorViewModel viewModel, Services.Monitor.MonitorService monitorService)
        {
            ViewModel = viewModel;
            DataContext = this;
            monitorService.Start();
            InitializeComponent();
        }

        /// <summary>
        /// Selects the cell on right-click so the Copy Cell context menu works immediately.
        /// </summary>
        private void DataGridCell_PreviewMouseRightButtonDown(object sender, MouseButtonEventArgs e)
        {
            if (sender is DataGridCell cell)
            {
                cell.IsSelected = true;
                cell.Focus();
            }
        }
    }
}
