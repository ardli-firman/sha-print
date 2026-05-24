using System;
using Wpf.Ui.Abstractions;

namespace ShaPrint.WpfApp.Services
{
    public class NavigationViewPageProvider : INavigationViewPageProvider
    {
        private readonly IServiceProvider _serviceProvider;

        public NavigationViewPageProvider(IServiceProvider serviceProvider)
        {
            _serviceProvider = serviceProvider;
        }

        public object? GetPage(Type pageType)
        {
            return _serviceProvider.GetService(pageType);
        }
    }
}
