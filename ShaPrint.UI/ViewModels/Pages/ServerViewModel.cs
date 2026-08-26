using Avalonia.Threading;
using CommunityToolkit.Mvvm.ComponentModel;
using CommunityToolkit.Mvvm.Input;
using ShaPrint.Core;
using ShaPrint.Core.Network;
using ShaPrint.Platform.Abstractions;
using ShaPrint.UI.Models;
using ShaPrint.UI.Services;
using System.Collections;
using System.Collections.Concurrent;
using System.Collections.ObjectModel;
using System.Text.Json;

namespace ShaPrint.UI.ViewModels.Pages;

/// <summary>One exposed printer. Selection is NOT stored on this item — the view's
/// <c>ListBox.SelectedItems</c> binds <see cref="ServerViewModel.SelectedPrinters"/> instead
/// (Task 6), so there is no <c>IsSelected</c> here (plan Task 5 rule).</summary>
public partial class PrinterItem : ObservableObject
{
    [ObservableProperty]
    private string _name;

    public PrinterItem(string name)
    {
        _name = name;
    }
}

/// <summary>One exposed scanner. Selection lives in <see cref="ServerViewModel.SelectedScanners"/>.</summary>
public partial class ScannerItem : ObservableObject
{
    [ObservableProperty]
    private string _name;

    public ScannerItem(string name)
    {
        _name = name;
    }
}

/// <summary>Persisted server configuration (HMAC-wrapped ServerConfig.json), same shape as
/// <c>ShaPrint.WpfApp</c>'s <c>ServerSavedConfig</c> so both shells share the file.</summary>
public class ServerSavedConfig
{
    public List<string> ExposedPrinters { get; set; } = new();
    public List<string> ExposedScanners { get; set; } = new();

    /// <summary>
    /// Stable server identity UUID. Null in files written before this feature was added;
    /// populated and persisted from the first save onward. Broadcast in discovery responses.
    /// </summary>
    public string? ServerId { get; set; }
}

/// <summary>
/// Server mode page. Migrated from <c>ShaPrint.WpfApp/ViewModels/Pages/ServerViewModel.cs</c>
/// (Task 5), rewired onto the shared engines from Task 4:
/// <list type="bullet">
/// <item><c>DiscoveryServer</c> + <c>PrintReceiver</c> (WpfApp services) ->
/// <see cref="DiscoveryServerService"/> + <see cref="PrintReceiverService"/> (DI singletons).</item>
/// <item><c>SpoolerApi.GetLocalPrinters</c> -> <see cref="IPrinterManager.GetLocalPrintersAsync"/>;</item>
/// <item><c>ScannerService</c> (WIA) -> <see cref="IScannerService.GetLocalScanners"/>;</item>
/// <item><c>FirewallManager.CheckAndAddFirewallRules</c> -> <see cref="IFirewallManager"/>;</item>
/// <item><c>DriverPackageService</c> -> optional <see cref="IDriverPackageProvider"/> (no
/// implementation is registered yet — see Startup).</item>
/// </list>
///
/// Platform services are resolved through <see cref="ViewModelSupport.Resolve{T}"/> so the VM
/// constructs on macOS/Linux too (before Task 7 there are no Unix backends): Start then reports
/// "server engine unavailable" instead of crashing the shell.
///
/// deferred (migrated later, not stubbed): <c>PrintMonitorService</c> (spooler queue probe) and
/// <c>MonitorTcpServer</c> (TCP 9878 status) from ShaPrint.WpfApp have no ShaPrint.UI equivalent
/// yet; the 9878 <see cref="MonitorService"/> queries will fail until they land.
/// </summary>
public partial class ServerViewModel : ObservableObject, IDisposable
{
    private static bool? _isUnitTest;
    public static bool IsUnitTest
    {
        get
        {
            if (!_isUnitTest.HasValue)
            {
                _isUnitTest = AppDomain.CurrentDomain.GetAssemblies()
                    .Any(a => a.FullName!.StartsWith("xunit", StringComparison.OrdinalIgnoreCase));
            }
            return _isUnitTest.Value;
        }
    }

    private readonly DiscoveryServerService? _discoveryServer;
    private readonly PrintReceiverService? _printReceiver;
    private readonly IFirewallManager? _firewallManager;
    private readonly IPrinterManager? _printerManager;
    private readonly IScannerService? _scannerService;
    private readonly string _configFile;

    public DateTime? ServerStartTime { get; private set; }

    /// <summary>Deferred: not fed yet — the DI-built <see cref="PrintReceiverService"/> has no
    /// per-instance log callbacks (WpfApp passed LogJob/LogError to its PrintReceiver ctor). Kept for API parity.</summary>
    public ConcurrentQueue<JobHistoryEntry> RecentJobs { get; } = new();
    public ConcurrentQueue<ServerErrorEntry> Errors { get; } = new();

    public List<string> ExposedPrinters { get; private set; } = new();
    public List<string> ExposedScanners { get; private set; } = new();

    public DiscoveryServerService? DiscoveryServer => _discoveryServer;

    /// <summary>Stable server identity. Null until the first <see cref="SaveConfiguration"/> call.</summary>
    public string? ServerId { get; private set; }

    [ObservableProperty]
    private bool _isRunning;

    [ObservableProperty]
    private string _statusText = "Status: Stopped";

    public ObservableCollection<PrinterItem> Printers { get; } = new();
    public ObservableCollection<ScannerItem> Scanners { get; } = new();
    public ObservableCollection<string> Logs { get; } = new();
    public string LogsText => string.Join(Environment.NewLine, Logs);

    /// <summary>Selection bound to the page's printer <c>ListBox.SelectedItems</c> (Task 6).
    /// Selection is NOT stored on <see cref="PrinterItem"/> — plan Task 5/6 rule.</summary>
    public IList SelectedPrinters { get; } = new List<PrinterItem>();
    public IList SelectedScanners { get; } = new List<ScannerItem>();

    public ServerViewModel(IServiceProvider serviceProvider)
    {
        // Optional / platform services (nullable on macOS/Linux before Task 7). DiscoveryServerService
        // and PrintReceiverService are registered in AddSharedServices but their constructors also
        // need platform backends, so they are resolved defensively here.
        _discoveryServer = ViewModelSupport.Resolve<DiscoveryServerService>(serviceProvider);
        _printReceiver = ViewModelSupport.Resolve<PrintReceiverService>(serviceProvider);
        _firewallManager = ViewModelSupport.Resolve<IFirewallManager>(serviceProvider);
        _printerManager = ViewModelSupport.Resolve<IPrinterManager>(serviceProvider);
        _scannerService = ViewModelSupport.Resolve<IScannerService>(serviceProvider);

        string dir = Path.Combine(Environment.GetFolderPath(Environment.SpecialFolder.LocalApplicationData), "ShaPrint");
        _configFile = Path.Combine(dir, "ServerConfig.json");

        AppLogger.OnLog += AppLogger_OnLog;

        LoadPrinters();
        LoadScanners();
        LoadConfiguration();
    }

    private void AppLogger_OnLog(string msg)
    {
        if (msg.Contains("[CLIENT]", StringComparison.OrdinalIgnoreCase)) return;

        void append() => AppendLog(msg);
        if (Avalonia.Application.Current is not null)
            Dispatcher.UIThread.Post(append);
        else
            append();
    }

    // AppLogger.Log already prefixes a timestamp; WpfApp double-timestamped here — dropped.
    private void AppendLog(string msg)
    {
        Logs.Insert(0, msg);
        if (Logs.Count > 200)
        {
            Logs.RemoveAt(Logs.Count - 1);
        }
        OnPropertyChanged(nameof(LogsText));
    }

    public void LogJob(JobHistoryEntry entry)
    {
        RecentJobs.Enqueue(entry);
        while (RecentJobs.Count > 50)
        {
            RecentJobs.TryDequeue(out _);
        }
    }

    public void LogError(ServerErrorEntry entry)
    {
        Errors.Enqueue(entry);
        while (Errors.Count > 50)
        {
            Errors.TryDequeue(out _);
        }
    }

    partial void OnIsRunningChanged(bool value)
    {
        OnPropertyChanged(nameof(ToggleButtonText));
        OnPropertyChanged(nameof(IsNotRunning));
    }

    public string ToggleButtonText => IsRunning ? "Stop Server" : "Start Server";
    public bool IsNotRunning => !IsRunning;

    private void LoadPrinters()
    {
        if (IsUnitTest || _printerManager == null) return;
        try
        {
            var printers = _printerManager.GetLocalPrintersAsync().GetAwaiter().GetResult();
            Printers.Clear();
            foreach (var p in printers)
            {
                Printers.Add(new PrinterItem(p.Name));
            }
        }
        catch (Exception ex)
        {
            AppLogger.Error("Failed to list printers during startup", ex);
        }
    }

    private void LoadScanners()
    {
        if (IsUnitTest || _scannerService == null) return;
        try
        {
            var scanners = _scannerService.GetLocalScanners();
            Scanners.Clear();
            foreach (var s in scanners)
            {
                Scanners.Add(new ScannerItem(s.Name));
            }
        }
        catch (Exception ex)
        {
            AppLogger.Error("Failed to list scanners during startup", ex);
        }
    }

    [RelayCommand]
    private void ToggleServer()
    {
        if (IsRunning)
        {
            StopServer();
        }
        else
        {
            _ = StartServerAsync();
        }
    }

    private async Task StartServerAsync(List<string>? printersOverride = null, List<string>? scannersOverride = null)
    {
        if (_discoveryServer == null || _printReceiver == null)
        {
            // macOS/Linux before Task 7: shared engines registered but no platform backend yet.
            StatusText = "Server engine unavailable on this platform.";
            return;
        }

        List<string> selectedPrinters;
        List<string> selectedScanners;

        if (IsUnitTest)
        {
            selectedPrinters = printersOverride ?? ExposedPrinters;
            selectedScanners = scannersOverride ?? ExposedScanners;
        }
        else
        {
            selectedPrinters = printersOverride ?? SelectedPrinters.OfType<PrinterItem>().Select(p => p.Name).ToList();
            selectedScanners = scannersOverride ?? SelectedScanners.OfType<ScannerItem>().Select(s => s.Name).ToList();

            if (selectedPrinters.Count == 0 && selectedScanners.Count == 0)
            {
                // WPF showed a MessageBox here — no dialog service in the shell yet, surface via status.
                StatusText = "Please select at least one printer or scanner to expose.";
                return;
            }

            ExposedPrinters = selectedPrinters;
            ExposedScanners = selectedScanners;
        }

        ServerStartTime = DateTime.UtcNow;

        // Clear history queues
        while (RecentJobs.TryDequeue(out _)) { }
        while (Errors.TryDequeue(out _)) { }

        _discoveryServer.SetExposedPrinters(selectedPrinters);
        _discoveryServer.SetExposedScanners(selectedScanners);

        // Driver sharing: no IDriverPackageProvider implementation is registered anywhere yet
        // (the Windows adapter wrapping ShaPrint.WpfApp's DriverPackageService is planned with
        // Task 7). Until then requests are rejected gracefully with "Driver sharing is disabled"
        // semantics — the same code path the protocol already uses for missing providers.
        _discoveryServer.SetDriverPackageProvider(null);
        _discoveryServer.SetDriverSharingEnabled(!IsUnitTest && (AppSettings.Current.DriverSharing?.Enabled ?? true));

        // deferred: PrintMonitorService.SetMonitoredPrinters(selectedPrinters) + Start()
        // deferred: MonitorTcpServer(new ServerStatusProvider(this)).Start() — TCP 9878 status.

        // Save configuration before starting so ServerId is persisted and broadcast correctly
        SaveConfiguration(selectedPrinters, selectedScanners);

        _discoveryServer.Start();
        _printReceiver.Start();

        // Ensure firewall rules are applied and logged whenever server starts
        try
        {
            if (_firewallManager != null)
            {
                await _firewallManager.EnsureFirewallRulesAsync();
            }
            else
            {
                AppLogger.Log("[SERVER] Firewall manager unavailable — skipping rule check (macOS/Linux pre-Task 7).");
            }
        }
        catch (Exception ex)
        {
            AppLogger.Error("[SERVER] Failed to apply firewall rules", ex);
        }

        IsRunning = true;
        StatusText = "Status: Running";
        AppendLog($"Server started — broadcasting {selectedPrinters.Count} printer(s) and {selectedScanners.Count} scanner(s).");
    }

    public void StopServer()
    {
        if (!IsRunning) return;

        _discoveryServer?.Stop();
        _printReceiver?.Stop();

        // deferred: _printMonitorService?.Stop(); _monitorTcpServer?.Stop();

        ServerStartTime = null;
        ExposedPrinters.Clear();
        ExposedScanners.Clear();

        IsRunning = false;
        StatusText = "Status: Stopped";

        Logs.Clear();
        OnPropertyChanged(nameof(LogsText));
    }

    private void LoadConfiguration()
    {
        if (IsUnitTest) return;
        if (!File.Exists(_configFile)) return;

        try
        {
            string raw = File.ReadAllText(_configFile);
            ConfigUnwrapResult result = CryptoHelper.UnwrapConfigWithHmac(raw, out string? json);

            if (result == ConfigUnwrapResult.Valid)
            {
                raw = json!;
            }
            else if (result == ConfigUnwrapResult.Tampered)
            {
                AppLogger.Error("[SERVER] Config file HMAC verification FAILED — possible tampering. Rejecting config.");
                return;
            }

            List<string>? savedPrinters = null;
            List<string>? savedScanners = null;

            try
            {
                var savedConfig = JsonSerializer.Deserialize<ServerSavedConfig>(raw);
                if (savedConfig != null)
                {
                    savedPrinters = savedConfig.ExposedPrinters;
                    savedScanners = savedConfig.ExposedScanners;
                    ServerId = savedConfig.ServerId;
                    _discoveryServer?.SetServerId(ServerId);
                }
            }
            catch
            {
                // Fallback to legacy List<string> for printers
                savedPrinters = JsonSerializer.Deserialize<List<string>>(raw);
            }

            // Seed the SelectedItems collections so the Task 6 ListBox reflects the saved state.
            if (savedPrinters != null)
            {
                foreach (var p in Printers)
                {
                    if (savedPrinters.Contains(p.Name) && !SelectedPrinters.Contains(p))
                    {
                        SelectedPrinters.Add(p);
                    }
                }
            }

            if (savedScanners != null)
            {
                foreach (var s in Scanners)
                {
                    if (savedScanners.Contains(s.Name) && !SelectedScanners.Contains(s))
                    {
                        SelectedScanners.Add(s);
                    }
                }
            }

            if ((savedPrinters != null && savedPrinters.Count > 0) || (savedScanners != null && savedScanners.Count > 0))
            {
                // Auto-restart with the persisted config (same as WpfApp). Fire-and-forget: the
                // awaited firewall step keeps the UI thread free.
                _ = StartServerAsync(savedPrinters, savedScanners);
            }
        }
        catch (Exception ex) { AppLogger.Error("Failed to load server configuration", ex); }
    }

    private void SaveConfiguration(List<string> printers, List<string> scanners)
    {
        if (IsUnitTest) return;
        try
        {
            var newServerId = ServerId;
            if (string.IsNullOrEmpty(newServerId))
            {
                newServerId = Guid.NewGuid().ToString("N");
            }

            var config = new ServerSavedConfig
            {
                ExposedPrinters = printers,
                ExposedScanners = scanners,
                ServerId = newServerId
            };
            string json = JsonSerializer.Serialize(config);
            string wrapped = CryptoHelper.WrapConfigWithHmac(json);
            File.WriteAllText(_configFile, wrapped);

            // Commit in-memory property and listener ONLY after successful write to disk
            ServerId = newServerId;
            _discoveryServer?.SetServerId(ServerId);
        }
        catch (Exception ex) { AppLogger.Error("Failed to save server configuration", ex); }
    }

    public void Dispose()
    {
        AppLogger.OnLog -= AppLogger_OnLog;
        if (IsRunning) StopServer();
    }
}