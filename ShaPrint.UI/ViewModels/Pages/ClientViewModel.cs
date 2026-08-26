using Avalonia.Threading;
using CommunityToolkit.Mvvm.ComponentModel;
using CommunityToolkit.Mvvm.Input;
using ShaPrint.Core;
using ShaPrint.Core.Network;
using ShaPrint.Platform.Abstractions;
using ShaPrint.UI.Services;
using System.Collections.ObjectModel;
using System.Text.Json;

#if WINDOWS
using ShaPrint.Platform.Windows;
#endif
// ShaPrint.Platform.Windows (legacy WPF layer) also exposes an INotificationService — pin the
// abstraction interface so the Windows TFM stays unambiguous.
using INotificationService = ShaPrint.Platform.Abstractions.INotificationService;

namespace ShaPrint.UI.ViewModels.Pages;

/// <summary>One discovered network printer on the Client page (same shape as WpfApp).</summary>
public partial class PrinterDisplayItem : ObservableObject
{
    public DiscoveryResponseMessage Server { get; }
    public PrinterInfo Printer { get; }

    [ObservableProperty]
    private bool _isInstalled;

    public bool IsVerified { get; }
    public string DisplayName => $"{(IsVerified ? "" : "[UNVERIFIED] ")}[{Server.ServerName}] {Printer.Name}";

    public PrinterDisplayItem(DiscoveryResponseMessage server, PrinterInfo printer, bool isInstalled, bool isVerified)
    {
        Server = server;
        Printer = printer;
        IsInstalled = isInstalled;
        IsVerified = isVerified;
    }
}

/// <summary>
/// Client mode page. Migrated from <c>ShaPrint.WpfApp/ViewModels/Pages/ClientViewModel.cs</c>
/// (Task 5), rewired onto the shared Task 4 services:
/// <list type="bullet">
/// <item><c>DiscoveryClient</c> -> <see cref="DiscoveryClientService"/> (real LAN scan);</item>
/// <item><c>VirtualPrinterManager.InstallPrinterAsync(vm, pipe, driver)</c> ->
/// <see cref="IVirtualPrinterManager.InstallPrinterAsync(string, string)"/> — the pipe name is
/// derived internally by the Windows adapter;</item>
/// <item><c>SpoolerApi.GetLocalPrinters</c> (installed check) ->
/// <see cref="IVirtualPrinterManager.GetInstalledVirtualPrinters"/>;</item>
/// <item><c>DriverNameResolver</c> is used only on the Windows TFM (ShaPrint.Platform.Windows,
/// net8.0-windows-only); the plain net8.0 TFM degrades to an exact-match fallback.</item>
/// <item><see cref="ServerReachabilityTracker"/> is built per-instance with delegates, exactly
/// like WpfApp — it is intentionally NOT DI-registered (Task 4).</item>
/// </list>
///
/// deferred (not stubbed): driver auto-provisioning (T11 — <c>DriverPackageManager</c> download +
/// <c>DriverInstaller</c> + WPF-UI confirmation dialogs) is left out: the confirmation/picker
/// dialogs have no Avalonia counterpart yet. The install flow therefore uses the #21 resolver
/// (exact/fuzzy local match, server-hint fallback) directly.
/// </summary>
public partial class ClientViewModel : ObservableObject, IDisposable
{
    private readonly DiscoveryClientService _discoveryClient;
    private readonly IVirtualPrinterManager? _virtualPrinterManager;
    private readonly INotificationService? _notificationService;
    private readonly string _configFile;

    private List<InstalledPrinterConfig> _installedPrinters = new();

#if WINDOWS
    private readonly List<PipeListener> _activeListeners = new();
#endif

    private volatile ServerReachabilityTracker? _tracker;

    [ObservableProperty]
    private string _targetIp = "";

    [ObservableProperty]
    private bool _isScanning;

    [ObservableProperty]
    private string _statusText = "Ready";

    [ObservableProperty]
    private PrinterDisplayItem? _selectedPrinter;

    public ObservableCollection<PrinterDisplayItem> DiscoveredPrinters { get; } = new();
    public ObservableCollection<string> Logs { get; } = new();
    public string LogsText => string.Join(Environment.NewLine, Logs);

    /// <summary>Locally installed printer drivers (from <see cref="IVirtualPrinterManager.GetInstalledDrivers"/>),
    /// refreshed before scanning so the driver picker/fallback has fresh data.</summary>
    public ObservableCollection<string> InstalledDrivers { get; } = new();

    /// <summary>Virtual printers this machine already hosts (from
    /// <see cref="IVirtualPrinterManager.GetInstalledVirtualPrinters"/>) — the ONLY source for
    /// "installed ShaPrint printers"; system drivers are NOT shown here.</summary>
    public ObservableCollection<string> InstalledVirtualPrinters { get; } = new();

    public ClientViewModel(DiscoveryClientService discoveryClient, IServiceProvider serviceProvider)
    {
        _discoveryClient = discoveryClient;
        _virtualPrinterManager = ViewModelSupport.Resolve<IVirtualPrinterManager>(serviceProvider);
        _notificationService = ViewModelSupport.Resolve<INotificationService>(serviceProvider);

        string dir = Path.Combine(Environment.GetFolderPath(Environment.SpecialFolder.LocalApplicationData), "ShaPrint");
        if (!Directory.Exists(dir)) Directory.CreateDirectory(dir);
        _configFile = Path.Combine(dir, "ClientConfig.json");

        AppLogger.OnLog += AppLogger_OnLog;

        RefreshInstalledPrinterLists();
        LoadConfiguration();
        InitializeTracker();
    }

    private void RefreshInstalledPrinterLists()
    {
        InstalledVirtualPrinters.Clear();
        if (_virtualPrinterManager != null)
        {
            foreach (var name in _virtualPrinterManager.GetInstalledVirtualPrinters())
            {
                InstalledVirtualPrinters.Add(name);
            }
        }
    }

    private void RefreshInstalledDrivers()
    {
        InstalledDrivers.Clear();
        if (_virtualPrinterManager != null)
        {
            foreach (var name in _virtualPrinterManager.GetInstalledDrivers())
            {
                InstalledDrivers.Add(name);
            }
        }
    }

    private void InitializeTracker()
    {
        _tracker = new ServerReachabilityTracker(
            configProvider: () => _installedPrinters,
            scanner: () => _discoveryClient.DiscoverServersAsync(targetIp: null, timeoutMs: 2000),
            onIdentityChanged: OnServerIdentityChanged,
            onSuspiciousMatch: OnSuspiciousMatch,
            debounceWindow: TimeSpan.FromSeconds(30)
        );

#if WINDOWS
        // Subscribe every existing + future PipeListener's OnServerUnreachable to the tracker.
        foreach (var l in _activeListeners) l.OnServerUnreachable += TriggerTrackerRescan;
#endif
    }

    private void TriggerTrackerRescan()
    {
        _ = Task.Run(async () =>
        {
            try
            {
                if (_tracker != null)
                {
                    await _tracker.RequestRescanAsync(ServerReachabilityTracker.RescanReason.PrintFailed, CancellationToken.None);
                }
            }
            catch (Exception ex)
            {
                AppLogger.Error("[CLIENT] Error during background reachability rescan", ex);
            }
        });
    }

#if WINDOWS
    private async Task RestartListenerAsync(InstalledPrinterConfig cfg)
    {
        // P2.3: pipe listener lifecycle must run on the UI thread; the Avalonia dispatcher is
        // the analog of the WPF one.
        if (Avalonia.Application.Current is not null && !Dispatcher.UIThread.CheckAccess())
        {
            await Dispatcher.UIThread.InvokeAsync(() => RestartListenerAsync(cfg));
            return;
        }

        var oldListener = _activeListeners.FirstOrDefault(l => l.PipeName == cfg.PipeName);
        if (oldListener != null)
        {
            oldListener.OnServerUnreachable -= TriggerTrackerRescan;
            await oldListener.StopAsync();
            _activeListeners.Remove(oldListener);
        }

        // P2.1: 200ms grace using async await Task.Delay to avoid blocking UI thread
        await Task.Delay(200);

        // Re-check if another listener was started for this pipe name during the delay
        var concurrentListener = _activeListeners.FirstOrDefault(l => l.PipeName == cfg.PipeName);
        if (concurrentListener != null)
        {
            concurrentListener.OnServerUnreachable -= TriggerTrackerRescan;
            await concurrentListener.StopAsync();
            _activeListeners.Remove(concurrentListener);
        }

        var fresh = new PipeListener(cfg.PipeName, cfg.ServerIp, cfg.TargetPrinterName, cfg.VirtualPrinterName);
        fresh.OnServerUnreachable += TriggerTrackerRescan;
        fresh.Start();
        _activeListeners.Add(fresh);
    }
#endif

    private void OnServerIdentityChanged(ServerIdentityChangedArgs args)
    {
        var cfg = FindConfigByPipeOrName(args.PipeName, args.VirtualPrinterName);
        if (cfg != null)
        {
#if WINDOWS
            _ = RestartListenerAsync(cfg);
#endif
        }

        SaveConfiguration();
        _notificationService?.ShowToast(
            "Server IP updated",
            $"Server '{args.ServerName}' IP updated: {args.OldIp} → {args.NewIp}");
        StatusText = $"Server IP updated: {args.OldIp} → {args.NewIp}";
    }

    private InstalledPrinterConfig? FindConfigByPipeOrName(string pipeName, string virtualPrinterName)
    {
        return _installedPrinters.FirstOrDefault(c =>
            (!string.IsNullOrEmpty(pipeName) && c.PipeName == pipeName) ||
            (string.IsNullOrEmpty(c.PipeName) && c.VirtualPrinterName == virtualPrinterName));
    }

    private void OnSuspiciousMatch(SuspiciousMatchArgs args)
    {
        _notificationService?.ShowToast(
            "Server identity changed",
            $"Server identity changed for '{args.ExpectedServerName}'. Please remove and reinstall this virtual printer.");
        StatusText = "Server identity changed — remove and reinstall the affected virtual printer.";
    }

    private void AppLogger_OnLog(string msg)
    {
        if (msg.Contains("[SERVER]", StringComparison.OrdinalIgnoreCase)) return;

        void append() => AppendLog(msg);
        if (Avalonia.Application.Current is not null)
            Dispatcher.UIThread.Post(append);
        else
            append();
    }

    private void AppendLog(string msg)
    {
        Logs.Insert(0, msg);
        if (Logs.Count > 200)
        {
            Logs.RemoveAt(Logs.Count - 1);
        }
        OnPropertyChanged(nameof(LogsText));
    }

    [RelayCommand]
    private async Task ScanAsync()
    {
        string? targetIp = null;
        if (!string.IsNullOrWhiteSpace(TargetIp))
        {
            if (!System.Net.IPAddress.TryParse(TargetIp.Trim(), out _))
            {
                StatusText = "Invalid IP Address format!";
                return;
            }
            targetIp = TargetIp.Trim();
        }

        IsScanning = true;
        StatusText = "Scanning...";
        DiscoveredPrinters.Clear();

        RefreshInstalledDrivers();
        RefreshInstalledPrinterLists();

        // Installed ShaPrint virtual printers (OS side) — replaces SpoolerApi.GetLocalPrinters().
        var installedOs = _virtualPrinterManager?.GetInstalledVirtualPrinters() ?? new List<string>();

        var discoveredServers = await _discoveryClient.DiscoverServersAsync(targetIp);

        foreach (var server in discoveredServers)
        {
            foreach (var printer in server.ExposedPrinters)
            {
                string virtualPrinterName = $"ShaPrint [{server.ServerName}] - {printer.Name}";
                bool isInstalledConfig = _installedPrinters.Any(p => p.VirtualPrinterName.Equals(virtualPrinterName, StringComparison.OrdinalIgnoreCase));
                bool isInstalledOs = installedOs.Contains(virtualPrinterName, StringComparer.OrdinalIgnoreCase);

                // Fallback backward compatibility check for old format: "ShaPrint - {PrinterName}"
                if (!isInstalledConfig && !isInstalledOs)
                {
                    isInstalledConfig = _installedPrinters.Any(p =>
                        p.TargetPrinterName.Equals(printer.Name, StringComparison.OrdinalIgnoreCase) &&
                        p.ServerIp.Equals(server.IpAddress));

                    string oldName = $"ShaPrint - {printer.Name}";
                    isInstalledOs = installedOs.Contains(oldName, StringComparer.OrdinalIgnoreCase);
                }

                bool isInstalled = isInstalledConfig || isInstalledOs;

                DiscoveredPrinters.Add(new PrinterDisplayItem(
                    server,
                    printer,
                    isInstalled,
                    !string.IsNullOrEmpty(server.HmacSignature)
                ));
            }
        }

        // Unicast rescan for any saved config entry that wasn't represented in the
        // broadcast results. This catches the "server IP changed, but we are still
        // pointed at the old IP via the saved config" case.
        var matchedVirtualNames = new HashSet<string>(
            DiscoveredPrinters.Select(p => p.DisplayName),
            StringComparer.OrdinalIgnoreCase);
        foreach (var cfg in _installedPrinters.ToList())
        {
            string displayName = $"[{ServerReachabilityTracker.ExtractServerName(cfg.VirtualPrinterName)}] {cfg.TargetPrinterName}";
            if (matchedVirtualNames.Contains(displayName)) continue;
            if (string.IsNullOrEmpty(cfg.ServerIp)) continue;

            var unicast = await _discoveryClient.DiscoverServersAsync(cfg.ServerIp, timeoutMs: 3000);
            if (unicast.Count == 0) continue;

            var match = unicast.FirstOrDefault(r =>
                (!string.IsNullOrEmpty(cfg.ServerId) && r.ServerId == cfg.ServerId) ||
                (string.IsNullOrEmpty(cfg.ServerId) && string.Equals(r.ServerName, ServerReachabilityTracker.ExtractServerName(cfg.VirtualPrinterName), StringComparison.OrdinalIgnoreCase)));

            if (match == null) continue;

            if (!string.Equals(match.IpAddress, cfg.ServerIp, StringComparison.Ordinal))
            {
                var oldIp = cfg.ServerIp;
                cfg.ServerIp = match.IpAddress;
                SaveConfiguration();

#if WINDOWS
                await RestartListenerAsync(cfg);
#endif
                _notificationService?.ShowToast(
                    "Server IP updated",
                    $"Server '{match.ServerName}' IP updated: {oldIp} → {match.IpAddress}");
            }
        }

        StatusText = $"Found {discoveredServers.Count} server(s).";
        IsScanning = false;
    }

    [RelayCommand]
    private async Task InstallSelectedAsync()
    {
        if (SelectedPrinter == null || SelectedPrinter.IsInstalled) return;
        if (_virtualPrinterManager == null)
        {
            StatusText = "Virtual printer backend unavailable on this platform.";
            return;
        }
        var item = SelectedPrinter;

        try
        {
            string serverName = Validators.ValidateServerName(item.Server.ServerName);
            string printerName = Validators.ValidatePrinterName(item.Printer.Name);
            string serverDriverHint = !string.IsNullOrEmpty(item.Printer.DriverName) ? item.Printer.DriverName : "Generic / Text Only";
            string serverIp = Validators.ValidateIpAddress(item.Server.IpAddress);

            string virtualPrinterName = $"ShaPrint [{serverName}] - {printerName}";

            if (_installedPrinters.Any(p => p.VirtualPrinterName == virtualPrinterName))
            {
                StatusText = "This printer is already installed!";
                return;
            }

            StatusText = "Installing...";

            // ── Driver resolution (issue #21 flow) ────────────────────────────
            // deferred: server-side driver auto-provisioning (DriverPackageManager download +
            // DriverInstaller + konfirmasi/picker ContentDialog WPF-UI) tidak dimigrasi — dialog
            // Avalonia belum ada (Task 6+). Fallback resolver dipakai persis jalur #21.
            RefreshInstalledDrivers();
            string? resolvedDriver = ResolveDriverName(serverDriverHint);
            if (resolvedDriver == null)
            {
                StatusText = "Installation cancelled.";
                return;
            }

            // Validate the resolved driver name to satisfy the same safety constraints as any
            // other user-supplied input.
            string driverName = Validators.ValidateDriverName(resolvedDriver);

#if WINDOWS
            string pipeName = DerivePipeName(virtualPrinterName);
            AppLogger.Log($"[CLIENT] Installing virtual printer '{virtualPrinterName}' with pipe '{pipeName}' using driver '{driverName}'...");
#else
            // No pipe backend on this platform yet (Unix layout lands with Task 7); the config
            // entry keeps PipeName empty and is tracked by ServerId/virtual name instead.
            string pipeName = string.Empty;
            AppLogger.Log($"[CLIENT] Installing virtual printer '{virtualPrinterName}' using driver '{driverName}'...");
#endif

            var result = await _virtualPrinterManager.InstallPrinterAsync(virtualPrinterName, driverName);

            if (result.Success)
            {
#if WINDOWS
                var listener = new PipeListener(pipeName, serverIp, printerName, virtualPrinterName);
                listener.OnServerUnreachable += TriggerTrackerRescan;
                listener.Start();
                _activeListeners.Add(listener);
#endif

                _installedPrinters.Add(new InstalledPrinterConfig
                {
                    VirtualPrinterName = virtualPrinterName,
                    PipeName = pipeName,
                    ServerIp = serverIp,
                    TargetPrinterName = printerName,
                    DriverName = driverName,
                    ServerId = item.Server.ServerId
                });
                SaveConfiguration();

                item.IsInstalled = true;
                RefreshInstalledPrinterLists();
                StatusText = "Installed successfully!";
            }
            else
            {
                StatusText = "Installation failed.";
                AppLogger.Error($"[CLIENT] Install failed: {result.ErrorMessage}");
            }
        }
        catch (ArgumentException ex)
        {
            StatusText = "Installation rejected.";
            AppLogger.Error($"[CLIENT] Input validation failed: {ex.Message}");
        }
        catch (Exception ex)
        {
            StatusText = "Installation failed.";
            AppLogger.Error($"[CLIENT] Install error: {ex.Message}", ex);
        }
    }

    /// <summary>
    /// Resolves the server-advertised driver name to a driver installed on THIS machine
    /// (issue #21: exact match → fuzzy match → server-hint fallback). The WPF picker and
    /// Generic/Text confirmation dialogs are deferred to the Avalonia view layer, so there is
    /// no "user cancelled" branch — resolution always yields a name here.
    /// </summary>
    private string? ResolveDriverName(string? serverDriverName)
    {
        var localDrivers = _virtualPrinterManager?.GetInstalledDrivers() ?? new List<string>();
        if (localDrivers.Count == 0)
        {
            AppLogger.Log("[CLIENT] No locally installed drivers enumerated — falling back to server hint.");
        }

        string hint = !string.IsNullOrWhiteSpace(serverDriverName) ? serverDriverName.Trim() : "Generic / Text Only";

#if WINDOWS
        // Full issue-#21 resolver (exact → normalized → model-token → weighted score).
        string? resolved = DriverNameResolver.Resolve(hint, localDrivers);
        return resolved ?? hint;
#else
        // Minimal cross-platform resolve — CUPS driver enumeration arrives with Task 7.
        return localDrivers.FirstOrDefault(d => string.Equals(d.Trim(), hint, StringComparison.OrdinalIgnoreCase)) ?? hint;
#endif
    }

#if WINDOWS
    /// <summary>
    /// Must match <see cref="ShaPrint.Platform.Windows.Adapters.WindowsVirtualPrinterManager.DerivePipeName"/>
    /// (internal on purpose). The client hosts the printer backend pipe with the SAME name the
    /// adapter used at install time; kept in sync manually until the adapter exposes it.
    /// </summary>
    private static string DerivePipeName(string virtualPrinterName)
    {
        var sb = new System.Text.StringBuilder(virtualPrinterName.Length);
        foreach (char c in virtualPrinterName)
        {
            sb.Append(char.IsLetterOrDigit(c) || c is '-' or '_' or '.' ? c : '_');
        }
        return $@"\\.\pipe\ShaPrint_{sb}";
    }
#endif

    [RelayCommand]
    private async Task DeleteSelectedAsync()
    {
        if (SelectedPrinter == null || !SelectedPrinter.IsInstalled) return;
        if (_virtualPrinterManager == null) return;
        var item = SelectedPrinter;

        string virtualPrinterName = $"ShaPrint [{item.Server.ServerName}] - {item.Printer.Name}";
        var config = _installedPrinters.FirstOrDefault(p => p.VirtualPrinterName.Equals(virtualPrinterName, StringComparison.OrdinalIgnoreCase));

        // Fallback for older configs
        if (config == null)
        {
            config = _installedPrinters.FirstOrDefault(p =>
                p.TargetPrinterName.Equals(item.Printer.Name, StringComparison.OrdinalIgnoreCase) &&
                p.ServerIp.Equals(item.Server.IpAddress));

            if (config != null)
            {
                virtualPrinterName = config.VirtualPrinterName;
            }
            else
            {
                // Fallback: check if the old format OS printer exists without config
                string oldName = $"ShaPrint - {item.Printer.Name}";
                var installedOs = _virtualPrinterManager.GetInstalledVirtualPrinters();
                if (installedOs.Contains(oldName, StringComparer.OrdinalIgnoreCase))
                {
                    virtualPrinterName = oldName;
                }
            }
        }

        StatusText = "Deleting...";

        var result = await _virtualPrinterManager.RemovePrinterAsync(virtualPrinterName);

        if (result.Success)
        {
            if (config != null)
            {
                _installedPrinters.Remove(config);
                SaveConfiguration();

#if WINDOWS
                var listener = _activeListeners.FirstOrDefault(l => l.PipeName == config.PipeName);
                if (listener != null)
                {
                    listener.OnServerUnreachable -= TriggerTrackerRescan;
                    listener.Stop();
                    _activeListeners.Remove(listener);
                }
#endif
            }

            item.IsInstalled = false;
            RefreshInstalledPrinterLists();
            StatusText = "Deleted successfully.";
        }
        else
        {
            StatusText = "Deletion failed.";
            AppLogger.Error($"[CLIENT] Delete failed: {result.ErrorMessage}");
        }
    }

    private void LoadConfiguration()
    {
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
                AppLogger.Error("[CLIENT] Config file HMAC verification FAILED — possible tampering. Rejecting config.");
                return;
            }

            var saved = JsonSerializer.Deserialize<List<InstalledPrinterConfig>>(raw);
            if (saved != null)
            {
                _installedPrinters = saved;
                foreach (var config in _installedPrinters)
                {
                    if (string.IsNullOrEmpty(config.ServerIp))
                    {
                        AppLogger.Log($"[CLIENT] Skipping invalid config entry: {config.VirtualPrinterName}");
                        continue;
                    }

#if WINDOWS
                    // On Unix there is no pipe backend, so PipeName is legitimately empty.
                    if (string.IsNullOrEmpty(config.PipeName))
                    {
                        AppLogger.Log($"[CLIENT] Skipping invalid config entry: {config.VirtualPrinterName}");
                        continue;
                    }

                    var listener = new PipeListener(config.PipeName, config.ServerIp, config.TargetPrinterName, config.VirtualPrinterName);
                    listener.Start();
                    _activeListeners.Add(listener);
#endif
                }

                if (_installedPrinters.Count > 0)
                {
                    _ = Task.Run(async () =>
                    {
                        try
                        {
                            await Task.Delay(2000);
                            if (_tracker == null) InitializeTracker();
                            if (_tracker != null)
                            {
                                await _tracker.RequestRescanAsync(ServerReachabilityTracker.RescanReason.Startup, CancellationToken.None);
                            }
                        }
                        catch (Exception ex)
                        {
                            AppLogger.Error("[CLIENT] Error during startup reachability rescan", ex);
                        }
                    });
                }
            }
        }
        catch (Exception ex) { AppLogger.Error("Failed to load client configuration", ex); }
    }

    private void SaveConfiguration()
    {
        try
        {
            string json = JsonSerializer.Serialize(_installedPrinters);
            string wrapped = CryptoHelper.WrapConfigWithHmac(json);
            File.WriteAllText(_configFile, wrapped);
        }
        catch (Exception ex) { AppLogger.Error("Failed to save client configuration", ex); }
    }

    // deferred: CancelDownloadCommand datang bersama pipeline driver auto-provisioning.

    public void StopClient()
    {
#if WINDOWS
        foreach (var listener in _activeListeners)
        {
            listener.Stop();
        }
        _activeListeners.Clear();
#endif
        Logs.Clear();
        OnPropertyChanged(nameof(LogsText));
    }

    public void Dispose()
    {
        AppLogger.OnLog -= AppLogger_OnLog;
        StopClient();
    }
}