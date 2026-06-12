using ShaPrint.WpfApp.ViewModels.Pages;
using System.Windows;
using System.Windows.Controls;
using System.Windows.Input;
using System.Windows.Media;

namespace ShaPrint.WpfApp.Views.Pages
{
    public partial class ScanPage : Page
    {
        public ScanViewModel ViewModel { get; }

        public ScanPage(ScanViewModel viewModel)
        {
            ViewModel = viewModel;
            DataContext = this;
            InitializeComponent();

            this.Loaded += ScanPage_Loaded;
        }

        private void ScanPage_Loaded(object sender, RoutedEventArgs e)
        {
            // Walk up the visual tree to find and disable any parent ScrollViewer
            // injected by the WPF UI NavigationView framework
            DependencyObject parent = VisualTreeHelper.GetParent(this);
            while (parent != null)
            {
                if (parent is ScrollViewer parentScroll)
                {
                    parentScroll.VerticalScrollBarVisibility = ScrollBarVisibility.Disabled;
                    parentScroll.HorizontalScrollBarVisibility = ScrollBarVisibility.Disabled;
                    break;
                }
                parent = VisualTreeHelper.GetParent(parent);
            }
        }

        private void PreviewScrollViewer_PreviewMouseWheel(object sender, MouseWheelEventArgs e)
        {
            var scrollViewer = sender as ScrollViewer;
            if (scrollViewer == null) return;

            // Check modifier keys
            if (Keyboard.Modifiers == ModifierKeys.Control)
            {
                // Ctrl + Scroll: Horizontal Scroll (Pan left/right)
                // Scroll Up (Delta > 0) -> Scroll Right (geser kanan)
                // Scroll Down (Delta < 0) -> Scroll Left (geser kiri)
                if (e.Delta > 0)
                {
                    scrollViewer.ScrollToHorizontalOffset(scrollViewer.HorizontalOffset + 40);
                }
                else
                {
                    scrollViewer.ScrollToHorizontalOffset(scrollViewer.HorizontalOffset - 40);
                }
                e.Handled = true;
            }
            else if (Keyboard.Modifiers == ModifierKeys.Shift)
            {
                // Shift + Scroll: Vertical Scroll (Pan up/down)
                if (e.Delta > 0)
                {
                    scrollViewer.ScrollToVerticalOffset(scrollViewer.VerticalOffset - 40);
                }
                else
                {
                    scrollViewer.ScrollToVerticalOffset(scrollViewer.VerticalOffset + 40);
                }
                e.Handled = true;
            }
            else
            {
                // Normal Scroll (No Modifier): Zoom In / Out
                if (e.Delta > 0)
                {
                    // Zoom In
                    if (ViewModel.ZoomInCommand.CanExecute(scrollViewer))
                    {
                        ViewModel.ZoomInCommand.Execute(scrollViewer);
                    }
                }
                else
                {
                    // Zoom Out
                    if (ViewModel.ZoomOutCommand.CanExecute(scrollViewer))
                    {
                        ViewModel.ZoomOutCommand.Execute(scrollViewer);
                    }
                }
                e.Handled = true;
            }
        }
    }
}
