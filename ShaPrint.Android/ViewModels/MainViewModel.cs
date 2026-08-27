using System.Collections.ObjectModel;
using CommunityToolkit.Mvvm.ComponentModel;
using CommunityToolkit.Mvvm.Input;
using ShaPrint.Android.Services;
using ShaPrint.Core;
using ShaPrint.Core.Network;

namespace ShaPrint.Android.ViewModels;

/// <summary>
/// Minimal Android shell ViewModel (Task 9): scan the LAN for ShaPrint servers, pick a
/// document via SAF and relay it to the selected printer. Exercises the two Android
/// services (<see cref="DiscoveryService"/> + <see cref="PrintRelayService"/>) end to end.
/// </summary>
public partial class MainViewModel : ObservableObject
{
    private readonly DiscoveryService _discovery;
    private readonly PrintRelayService _relay;

    [ObservableProperty]
    private string _statusText = "Ready — tap Scan to find ShaPrint servers.";

    [ObservableProperty]
    private bool _isBusy;

    [ObservableProperty]
    private string? _selectedPrinter;

    public ObservableCollection<string> AvailablePrinters { get; } = new();

    public MainViewModel(DiscoveryService discovery, PrintRelayService relay)
    {
        _discovery = discovery;
        _relay = relay;
    }

    [RelayCommand]
    private async Task ScanLanAsync()
    {
        if (IsBusy)
        {
            return;
        }

        IsBusy = true;
        AvailablePrinters.Clear();
        StatusText = "Scanning the LAN…";
        try
        {
            var servers = await _discovery.DiscoverServersAsync();

            var printers = servers
                .SelectMany(s => s.ExposedPrinters ?? new List<PrinterInfo>())
                .Select(p => p.Name)
                .Distinct(StringComparer.OrdinalIgnoreCase)
                .OrderBy(n => n, StringComparer.OrdinalIgnoreCase)
                .ToList();

            foreach (var printer in printers)
            {
                AvailablePrinters.Add(printer);
            }

            StatusText = printers.Count == 0
                ? "No printers found — make sure a ShaPrint server is running and reachable."
                : $"Found {printers.Count} printer(s) on {servers.Count} server(s).";
        }
        catch (Exception ex)
        {
            StatusText = $"Scan failed: {ex.Message}";
            AppLogger.Error("LAN scan failed", ex);
        }
        finally
        {
            IsBusy = false;
        }
    }

    [RelayCommand]
    private async Task PickAndSendAsync()
    {
        if (IsBusy)
        {
            return;
        }

        if (string.IsNullOrWhiteSpace(SelectedPrinter))
        {
            StatusText = "Select a printer first (Scan LAN), then pick a file.";
            return;
        }

        IsBusy = true;
        try
        {
            StatusText = "Waiting for file selection…";
            PickedDocument? picked = await _relay.PickDocumentAsync();
            if (picked == null)
            {
                StatusText = "File selection cancelled or unreadable.";
                return;
            }

            if (picked.Data.Length == 0)
            {
                StatusText = "Selected file is empty — nothing to send.";
                return;
            }

            if (picked.Data.Length > Constants.MaxPrintJobBytes)
            {
                StatusText = $"File too large ({picked.Data.Length:N0} bytes; max {Constants.MaxPrintJobBytes:N0}).";
                return;
            }

            StatusText = $"Sending {picked.Data.Length:N0} bytes to printer '{SelectedPrinter}'…";
            bool sent = await _relay.SendAsync(SelectedPrinter, picked.Data, picked.DocumentName);

            StatusText = sent
                ? $"Job accepted by the server for '{SelectedPrinter}'."
                : "Send failed — see the console/log for details.";
        }
        catch (Exception ex)
        {
            StatusText = $"Send failed: {ex.Message}";
            AppLogger.Error("Send failed", ex);
        }
        finally
        {
            IsBusy = false;
        }
    }
}
