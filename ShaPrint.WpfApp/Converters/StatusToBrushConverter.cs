using System;
using System.Globalization;
using System.Windows.Data;
using System.Windows.Media;

namespace ShaPrint.WpfApp.Converters
{
    /// <summary>
    /// Converts status strings to themed brushes for colored status pills.
    /// Supports printer/scanner/job status values.
    /// </summary>
    public class StatusToBrushConverter : IValueConverter
    {
        private static readonly Brush _onlineBrush = new SolidColorBrush(Color.FromRgb(34, 197, 94));    // green
        private static readonly Brush _errorBrush = new SolidColorBrush(Color.FromRgb(239, 68, 68));     // red
        private static readonly Brush _warningBrush = new SolidColorBrush(Color.FromRgb(245, 158, 11));  // orange
        private static readonly Brush _infoBrush = new SolidColorBrush(Color.FromRgb(59, 130, 246));     // blue
        private static readonly Brush _defaultBrush = new SolidColorBrush(Color.FromRgb(156, 163, 175)); // gray

        private static readonly Brush _onlineTextBrush = Brushes.White;
        private static readonly Brush _errorTextBrush = Brushes.White;
        private static readonly Brush _warningTextBrush = Brushes.White;
        private static readonly Brush _infoTextBrush = Brushes.White;
        private static readonly Brush _defaultTextBrush = new SolidColorBrush(Color.FromRgb(55, 65, 81)); // dark gray

        public object? Convert(object? value, Type targetType, object? parameter, CultureInfo culture)
        {
            if (value is not string status)
                return _defaultBrush;

            return status.ToLowerInvariant() switch
            {
                "online" or "available" or "completed" => _onlineBrush,
                "error" or "failed" => _errorBrush,
                "idle" or "warning" => _warningBrush,
                "inuse" or "printing" or "in_use" => _infoBrush,
                _ => _defaultBrush
            };
        }

        public object? ConvertBack(object? value, Type targetType, object? parameter, CultureInfo culture)
            => throw new NotImplementedException();
    }

    /// <summary>
    /// Converts status strings to appropriate text (foreground) brushes for colored status pills.
    /// </summary>
    public class StatusToTextBrushConverter : IValueConverter
    {
        public object? Convert(object? value, Type targetType, object? parameter, CultureInfo culture)
        {
            if (value is not string status)
                return _defaultTextBrush;

            return status.ToLowerInvariant() switch
            {
                "online" or "available" or "completed"
                    or "error" or "failed"
                    or "idle" or "warning"
                    or "inuse" or "printing" or "in_use" => _whiteBrush,
                _ => _defaultTextBrush
            };
        }

        public object? ConvertBack(object? value, Type targetType, object? parameter, CultureInfo culture)
            => throw new NotImplementedException();

        private static readonly Brush _whiteBrush = Brushes.White;
        private static readonly Brush _defaultTextBrush = new SolidColorBrush(Color.FromRgb(55, 65, 81));
    }
}
