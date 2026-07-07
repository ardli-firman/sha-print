using System;
using System.Globalization;
using System.Windows;
using System.Windows.Data;

namespace ShaPrint.WpfApp.Converters
{
    public class IntToVisibilityConverter : IValueConverter
    {
        public object Convert(object value, Type targetType, object parameter, CultureInfo culture)
        {
            bool inverse = parameter is string str && str.Equals("Inverse", StringComparison.OrdinalIgnoreCase);

            if (value is int intValue)
            {
                bool visible = intValue > 0;
                if (inverse) visible = !visible;
                return visible ? Visibility.Visible : Visibility.Collapsed;
            }

            // If binding fails or value is null, default to collapsed (or visible if inverse)
            return inverse ? Visibility.Visible : Visibility.Collapsed;
        }

        public object ConvertBack(object value, Type targetType, object parameter, CultureInfo culture)
        {
            throw new NotImplementedException();
        }
    }
}
