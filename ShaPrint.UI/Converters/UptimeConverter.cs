using Avalonia.Data.Converters;
using System.Globalization;

namespace ShaPrint.UI.Converters;

/// <summary>
/// Formats <c>ServerStatusPayload.UptimeSeconds</c> (and <see cref="ViewModels.Pages.ServerNode.UptimeSeconds"/>)
/// as a compact "Xd Yh Zm" string. Mirrors the formatting already used by
/// <see cref="ViewModels.Pages.ServerNode.UptimeText"/> so the Monitor page shows the same
/// convention as the WPF app (Task 6 plan: no brush converters needed here).
///
/// Exposed as <see cref="Instance"/> for <c>{x:Static conv:UptimeConverter.Instance}</c>.
/// </summary>
public class UptimeConverter : IValueConverter
{
    public static readonly UptimeConverter Instance = new();

    public object? Convert(object? value, Type targetType, object? parameter, CultureInfo culture)
    {
        long seconds = value switch
        {
            long l => l,
            int i => i,
            double d => (long)d,
            _ => 0
        };
        return Format(seconds);
    }

    public object? ConvertBack(object? value, Type targetType, object? parameter, CultureInfo culture)
    {
        throw new NotSupportedException("UptimeConverter is one-way only.");
    }

    internal static string Format(long totalSeconds)
    {
        if (totalSeconds <= 0) return "0s";

        var span = TimeSpan.FromSeconds(totalSeconds);
        if (span.TotalDays >= 1)
            return $"{(int)span.TotalDays}d {span.Hours}h {span.Minutes}m";
        if (span.TotalHours >= 1)
            return $"{span.Hours}h {span.Minutes}m {span.Seconds}s";
        return $"{span.Minutes}m {span.Seconds}s";
    }
}
