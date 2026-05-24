using System.Windows;
using System.Windows.Controls;
using System.Windows.Input;
using System.Windows.Media;
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

            // When the page is loaded into the NavigationView, disable any parent ScrollViewer
            // that WPF UI's NavigationView may inject around our page content.
            this.Loaded += SettingsPage_Loaded;
        }

        private void SettingsPage_Loaded(object sender, RoutedEventArgs e)
        {
            // Walk up the visual tree to find and disable any parent ScrollViewer
            // injected by the WPF UI NavigationView framework
            DependencyObject parent = VisualTreeHelper.GetParent(this);
            while (parent != null)
            {
                if (parent is ScrollViewer parentScroll && parentScroll != RootScrollViewer)
                {
                    parentScroll.VerticalScrollBarVisibility = ScrollBarVisibility.Disabled;
                    break;
                }
                parent = VisualTreeHelper.GetParent(parent);
            }
        }

        /// <summary>
        /// Intercepts mouse wheel events via the tunneling (Preview) route so that WPF UI controls
        /// (CardControl, CardAction, ToggleSwitch) cannot swallow them. Manually scrolls our
        /// ScrollViewer and marks the event as handled.
        /// </summary>
        private void RootScrollViewer_PreviewMouseWheel(object sender, MouseWheelEventArgs e)
        {
            var scrollViewer = (ScrollViewer)sender;
            scrollViewer.ScrollToVerticalOffset(scrollViewer.VerticalOffset - e.Delta);
            e.Handled = true;
        }
    }
}
