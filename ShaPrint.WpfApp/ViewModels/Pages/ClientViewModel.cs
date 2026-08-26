using CommunityToolkit.Mvvm.ComponentModel;
using CommunityToolkit.Mvvm.Input;
using ShaPrint.Core;
using ShaPrint.Core.Network;
using ShaPrint.Core.Abstractions;
using ShaPrint.Client;
using ShaPrint.Platform.Windows;
using ShaPrint.WpfApp.Views.Pages;
using System;
using System.Collections.Generic;
using System.Collections.ObjectModel;
using System.IO;
using System.Linq;
using System.Text.Json;
using System.Threading;
using System.Threading.Tasks;
using System.Windows;
using Wpf.Ui;
using Wpf.Ui.Controls;

namespace ShaPrint.WpfApp.ViewModels.Pages
{
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

    public class InstalledPrinterConfig
    {
        public string VirtualPrinterName { get; set; } = string.Empty;
        public string PipeName { get; set; } = string.Empty;
        public string ServerIp { get; set; } = string.Empty;
        public string TargetPrinterName { get; set; } = string.Empty;
        public string DriverName { get; set; } = string.Empty;

        /// <summary>
        /// Stable per-server UUID captured from the discovery response at install time.
        /// Null for installs from pre-ServerId servers. Used by ServerReachabilityTracker
        /// to match this entry against a discovered server whose IP may have changed.
        /// </summary>
        public string? ServerId { get; set; }
    }

    public partial class ClientViewModel : ObservableObject, IDisposable
    {
        private readonly DiscoveryClient _discoveryClient;
        private readonly INavigationService _navigationService;
        private readonly ISnackbarService _snackbarService;
        private readonly IContentDialogService _contentDialogService;
        private readonly string _configFile;

        // Driver auto-provisioning (client-side)
        private readonly DriverPackageManager _driverPackageManager;
        private readonly DriverInstaller _driverInstaller;
        private CancellationTokenSource? _downloadCts;

        private List<InstalledPrinterConfig> _installedPrinters = new();
        private List<PipeListener> _activeListeners = new();
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

        public ClientViewModel(
            INavigationService navigationService,
            ISnackbarService snackbarService,
            IContentDialogService contentDialogService)
        {
            _navigationService = navigationService;
            _snackbarService = snackbarService;
            _contentDialogService = contentDialogService;
            _discoveryClient = new DiscoveryClient();
            _driverPackageManager = new DriverPackageManager();
            _driverInstaller = new DriverInstaller(new RealProcessRunner());
            
            string dir = Path.Combine(Environment.GetFolderPath(Environment.SpecialFolder.LocalApplicationData), "ShaPrint");
            if (!Directory.Exists(dir)) Directory.CreateDirectory(dir);
            _configFile = Path.Combine(dir, "ClientConfig.json");

            AppLogger.OnLog += AppLogger_OnLog;

            LoadConfiguration();
            InitializeTracker();
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

            // Subscribe every existing + future PipeListener's OnServerUnreachable to the tracker.
            foreach (var l in _activeListeners) l.OnServerUnreachable += TriggerTrackerRescan;
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

        private async Task RestartListenerAsync(InstalledPrinterConfig cfg)
        {
            // P2.3: Must run on UI Thread!
            if (Application.Current?.Dispatcher != null && !Application.Current.Dispatcher.CheckAccess())
            {
                await Application.Current.Dispatcher.InvokeAsync(() => RestartListenerAsync(cfg));
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

        private void OnServerIdentityChanged(ServerIdentityChangedArgs args)
        {
            var cfg = _installedPrinters.FirstOrDefault(c => c.PipeName == args.PipeName);
            if (cfg != null)
            {
                _ = RestartListenerAsync(cfg);
            }

            SaveConfiguration();
            Application.Current?.Dispatcher.InvokeAsync(() =>
            {
                _snackbarService.Show(
                    "Server IP updated",
                    $"Server '{args.ServerName}' IP updated: {args.OldIp} → {args.NewIp}",
                    ControlAppearance.Info,
                    new Wpf.Ui.Controls.SymbolIcon(Wpf.Ui.Controls.SymbolRegular.ArrowSync24),
                    TimeSpan.FromSeconds(5));
            });
        }

        private void OnSuspiciousMatch(SuspiciousMatchArgs args)
        {
            Application.Current?.Dispatcher.InvokeAsync(() =>
            {
                _snackbarService.Show(
                    "Server identity changed",
                    $"Server identity changed for '{args.ExpectedServerName}'. Please remove and reinstall this virtual printer.",
                    ControlAppearance.Caution,
                    new Wpf.Ui.Controls.SymbolIcon(Wpf.Ui.Controls.SymbolRegular.Warning24),
                    TimeSpan.FromSeconds(7));
            });
        }

        private void AppLogger_OnLog(string msg)
        {
            if (msg.Contains("[SERVER]", StringComparison.OrdinalIgnoreCase)) return;

            Application.Current.Dispatcher.InvokeAsync(() =>
            {
                Logs.Insert(0, $"[{DateTime.Now:HH:mm:ss}] {msg}");
                if (Logs.Count > 200)
                {
                    Logs.RemoveAt(Logs.Count - 1);
                }
                OnPropertyChanged(nameof(LogsText));
            });
        }

        [RelayCommand]
        private async Task ScanAsync()
        {
            string? targetIp = null;
            if (!string.IsNullOrWhiteSpace(TargetIp))
            {
                if (!System.Net.IPAddress.TryParse(TargetIp.Trim(), out _))
                {
                    System.Windows.MessageBox.Show("Invalid IP Address format!", "Error", System.Windows.MessageBoxButton.OK, System.Windows.MessageBoxImage.Error);
                    return;
                }
                targetIp = TargetIp.Trim();
            }

            IsScanning = true;
            StatusText = "Scanning...";
            DiscoveredPrinters.Clear();

            var localPrinters = SpoolerApi.GetLocalPrinters();
            var discoveredServers = await _discoveryClient.DiscoverServersAsync(targetIp);

            foreach (var server in discoveredServers)
            {
                foreach (var printer in server.ExposedPrinters)
                {
                    string virtualPrinterName = $"ShaPrint [{server.ServerName}] - {printer.Name}";
                    bool isInstalledConfig = _installedPrinters.Any(p => p.VirtualPrinterName.Equals(virtualPrinterName, StringComparison.OrdinalIgnoreCase));
                    bool isInstalledOs = localPrinters.Contains(virtualPrinterName, StringComparer.OrdinalIgnoreCase);

                    // Fallback backward compatibility check for old format: "ShaPrint - {PrinterName}"
                    if (!isInstalledConfig && !isInstalledOs)
                    {
                        isInstalledConfig = _installedPrinters.Any(p => 
                            p.TargetPrinterName.Equals(printer.Name, StringComparison.OrdinalIgnoreCase) && 
                            p.ServerIp.Equals(server.IpAddress));
                            
                        string oldName = $"ShaPrint - {printer.Name}";
                        isInstalledOs = localPrinters.Contains(oldName, StringComparer.OrdinalIgnoreCase);
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
                    
                    await RestartListenerAsync(cfg);

                    _snackbarService.Show(
                        "Server IP updated",
                        $"Server '{match.ServerName}' IP updated: {oldIp} → {match.IpAddress}",
                        ControlAppearance.Info,
                        new Wpf.Ui.Controls.SymbolIcon(Wpf.Ui.Controls.SymbolRegular.ArrowSync24),
                        TimeSpan.FromSeconds(5));
                }
            }

            StatusText = $"Found {discoveredServers.Count} server(s).";
            IsScanning = false;
        }

        [RelayCommand]
        private async Task InstallSelectedAsync()
        {
            if (SelectedPrinter == null || SelectedPrinter.IsInstalled) return;
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
                    System.Windows.MessageBox.Show("This printer is already installed!", "Info", System.Windows.MessageBoxButton.OK, System.Windows.MessageBoxImage.Information);
                    return;
                }

                StatusText = "Installing...";

                // ── Driver auto-provisioning flow (T11) ────────────────────────────
                // Check if server offers driver provisioning
                string? resolvedDriver = null;
                bool provisioningAttempted = false;

                if (item.Printer.DriverAvailable && !string.IsNullOrEmpty(item.Printer.DriverPackageId))
                {
                    // Fast path: silently check if local driver already exists without popping up UI
                    var localDrivers = await Task.Run(() => VirtualPrinterManager.GetInstalledDrivers());
                    string? existingLocal = DriverNameResolver.Resolve(serverDriverHint, localDrivers);

                    if (existingLocal != null)
                    {
                        resolvedDriver = existingLocal;
                        AppLogger.Log($"[CLIENT] Local driver already exists ('{resolvedDriver}') — skipping provisioning.");
                    }
                    else
                    {
                        // No local match — attempt provisioning
                        provisioningAttempted = true;

                        // T10: Show confirmation dialog (default ON — security vs UX)
                        bool confirmed = await ShowDriverProvisioningConfirmAsync(
                            item.Printer.Name, serverName, serverDriverHint,
                            item.Printer.DriverSizeBytes, item.Printer.DriverPackageId!);

                        if (!confirmed)
                        {
                            AppLogger.Log("[CLIENT] User cancelled driver provisioning — falling back to #21 resolver.");
                            _snackbarService.Show(
                                "Driver provisioning cancelled",
                                "Falling back to manual driver resolution.",
                                ControlAppearance.Caution,
                                new Wpf.Ui.Controls.SymbolIcon(Wpf.Ui.Controls.SymbolRegular.Warning24),
                                TimeSpan.FromSeconds(5));
                        }
                        else
                        {
                            // T12: Progress reporting via snackbar
                            _snackbarService.Show(
                                "Downloading driver",
                                $"Downloading driver package from server ({FormatBytes(item.Printer.DriverSizeBytes)})...",
                                ControlAppearance.Info,
                                new Wpf.Ui.Controls.SymbolIcon(Wpf.Ui.Controls.SymbolRegular.ArrowDownload24),
                                TimeSpan.FromSeconds(10));

                            var progress = new Progress<double>(pct =>
                            {
                                StatusText = $"Downloading driver: {pct:P0}...";
                            });

                            // H1/H2: Download driver package with cancellation support
                            bool retryDownload = true;
                            while (retryDownload)
                            {
                                retryDownload = false;
                                _downloadCts?.Dispose();
                                _downloadCts = new CancellationTokenSource();

                                var downloadResult = await _driverPackageManager.DownloadDriverPackageAsync(
                                    serverIp, printerName, item.Printer.DriverPackageId!,
                                    item.Printer.DriverSizeBytes, progress, _downloadCts.Token);

                                _downloadCts.Dispose();
                                _downloadCts = null;

                                if (!downloadResult.Success)
                                {
                                    // H1: Cancel/Retry dialog on timeout
                                    if (downloadResult.TimedOut)
                                    {
                                        var retryDialog = new ContentDialog
                                        {
                                            Title = "Driver download timed out",
                                            Content = "Driver transfer timed out — the server may be unresponsive.\n\nWould you like to retry?",
                                            PrimaryButtonText = "Retry",
                                            CloseButtonText = "Cancel",
                                            DefaultButton = ContentDialogButton.Primary,
                                        };

                                        try
                                        {
                                            var retryResult = await _contentDialogService.ShowAsync(retryDialog, CancellationToken.None);
                                            if (retryResult == ContentDialogResult.Primary)
                                            {
                                                AppLogger.Log("[CLIENT] User chose Retry after download timeout.");
                                                retryDownload = true;
                                                continue;
                                            }
                                            else
                                            {
                                                AppLogger.Log("[CLIENT] User chose Cancel after download timeout — falling back.");
                                            }
                                        }
                                        catch (Exception ex)
                                        {
                                            AppLogger.Error("[CLIENT] Retry dialog failed: " + ex.Message);
                                        }
                                    }

                                    AppLogger.Log($"[CLIENT] Driver download failed: {downloadResult.ErrorMessage}");
                                    _snackbarService.Show(
                                        "Driver download failed",
                                        $"{downloadResult.ErrorMessage} Falling back to manual resolution.",
                                        ControlAppearance.Danger,
                                        new SymbolIcon(SymbolRegular.ErrorCircle24),
                                        TimeSpan.FromSeconds(7));
                                }
                                else
                                {
                                    // H4: Resolve .inf path deterministically
                                    string? manifestInfName = null;
                                    bool archValid = true;
                                    string? archError = null;
                                    try
                                    {
                                        string manifestPath = Path.Combine(downloadResult.PackageDirectory!, "manifest.json");
                                        if (File.Exists(manifestPath))
                                        {
                                            var manifestJson = File.ReadAllText(manifestPath);
                                            var manifest = JsonSerializer.Deserialize<ShaPrint.Core.Network.DriverPackageManifest>(manifestJson);
                                            manifestInfName = manifest?.InfName;
                                            if (manifest != null && !DriverSafetyGuard.ValidateArchitecture(manifest.Architecture, out archError))
                                            {
                                                archValid = false;
                                            }
                                        }
                                    }
                                    catch (Exception ex)
                                    {
                                        AppLogger.Log($"[CLIENT] Could not read manifest.json for InfName: {ex.Message}");
                                    }

                                    if (!archValid)
                                    {
                                        AppLogger.Log($"[CLIENT] Driver provisioning aborted by Safety Guard: {archError}");
                                        _snackbarService.Show(
                                            "Incompatible driver architecture",
                                            $"{archError} Manual installation required.",
                                            ControlAppearance.Danger,
                                            new SymbolIcon(SymbolRegular.ErrorCircle24),
                                            TimeSpan.FromSeconds(7));
                                    }
                                    else
                                    {
                                        string? infPath = _driverPackageManager.ResolveInfPath(downloadResult.PackageDirectory!, manifestInfName);

                                        if (infPath == null)
                                        {
                                            AppLogger.Log("[CLIENT] Could not resolve .inf file — ambiguous or missing. Falling back.");
                                            _snackbarService.Show(
                                                "Driver package invalid",
                                                "Multiple driver files found or no .inf file in package — manual installation required. Falling back.",
                                                ControlAppearance.Danger,
                                                new SymbolIcon(SymbolRegular.ErrorCircle24),
                                                TimeSpan.FromSeconds(7));
                                        }
                                        else
                                        {
                                            // H6: Install driver with inbox fallback
                                            StatusText = "Installing driver...";
                                            var installResult = await _driverInstaller.InstallDriverFromInfAsync(infPath, serverDriverHint);

                                            if (installResult.Success)
                                            {
                                                AppLogger.Log("[CLIENT] Server-provided driver installed successfully.");
                                                _snackbarService.Show(
                                                    "Driver installed",
                                                    $"Driver '{serverDriverHint}' installed from server.",
                                                    ControlAppearance.Success,
                                                    new SymbolIcon(SymbolRegular.Checkmark24),
                                                    TimeSpan.FromSeconds(3));

                                                // Re-resolve to get the exact driver name
                                                var updatedDrivers = await Task.Run(() => VirtualPrinterManager.GetInstalledDrivers());
                                                resolvedDriver = DriverNameResolver.Resolve(serverDriverHint, updatedDrivers) ?? serverDriverHint;
                                            }
                                            else
                                            {
                                                AppLogger.Log($"[CLIENT] Driver install failed: {installResult.ErrorMessage}");
                                                _snackbarService.Show(
                                                    "Driver installation failed",
                                                    $"{installResult.ErrorMessage} Falling back to manual resolution.",
                                                    ControlAppearance.Danger,
                                                    new SymbolIcon(SymbolRegular.ErrorCircle24),
                                                    TimeSpan.FromSeconds(7));
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }

                // If provisioning wasn't attempted or didn't resolve, fall back to existing #21 flow
                if (resolvedDriver == null)
                {
                    if (!provisioningAttempted)
                    {
                        // No provisioning available — use existing flow
                        resolvedDriver = await ResolveDriverNameAsync(serverDriverHint);
                    }
                    else
                    {
                        // Provisioning failed — fallback to #21 resolver with explicit message
                        AppLogger.Log("[CLIENT] Provisioning failed — falling back to #21 driver name resolver.");
                        _snackbarService.Show(
                            "Using manual driver resolution",
                            "Automatic driver provisioning failed. Please select a driver manually.",
                            ControlAppearance.Caution,
                            new Wpf.Ui.Controls.SymbolIcon(Wpf.Ui.Controls.SymbolRegular.Warning24),
                            TimeSpan.FromSeconds(7));
                        resolvedDriver = await ResolveDriverNameAsync(serverDriverHint);
                    }
                }

                if (resolvedDriver == null)
                {
                    StatusText = "Installation cancelled.";
                    return;
                }

                // Validate the resolved driver name (may come from picker / fallback) to ensure
                // it satisfies the same safety constraints as any other user-supplied input.
                string driverName = Validators.ValidateDriverName(resolvedDriver);

                string pipeName = $@"\\.\pipe\shaprint_{Guid.NewGuid():N}";
                AppLogger.Log($"[CLIENT] Installing virtual printer '{virtualPrinterName}' with pipe '{pipeName}' using driver '{driverName}'...");

                var result = await VirtualPrinterManager.InstallPrinterAsync(virtualPrinterName, pipeName, driverName);

                if (result.Success)
                {
                    var listener = new PipeListener(pipeName, serverIp, printerName, virtualPrinterName);
                    listener.OnServerUnreachable += TriggerTrackerRescan;
                    listener.Start();
                    _activeListeners.Add(listener);

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
                    StatusText = "Installed successfully!";
                    _snackbarService.Show("Printer Installed", $"'{virtualPrinterName}' has been installed.", ControlAppearance.Success, new Wpf.Ui.Controls.SymbolIcon(Wpf.Ui.Controls.SymbolRegular.Checkmark24), TimeSpan.FromSeconds(3));
                }
                else
                {
                    StatusText = "Installation failed.";
                    System.Windows.MessageBox.Show($"Failed to install printer. Please ensure you run this application as Administrator.\n\nDetails: {result.ErrorMessage}",
                        "Error", System.Windows.MessageBoxButton.OK, System.Windows.MessageBoxImage.Error);
                }
            }
            catch (ArgumentException ex)
            {
                StatusText = "Installation rejected.";
                System.Windows.MessageBox.Show($"Security: {ex.Message}\n\nThe server may be sending invalid or malicious data.",
                    "Security Warning", System.Windows.MessageBoxButton.OK, System.Windows.MessageBoxImage.Warning);
                AppLogger.Error($"[CLIENT] Input validation failed: {ex.Message}");
            }
        }

        /// <summary>
        /// T10: Shows the driver provisioning confirmation dialog (default ON).
        /// Returns true if user confirms installation, false if cancelled.
        /// </summary>
        private async Task<bool> ShowDriverProvisioningConfirmAsync(
            string printerName, string serverName, string driverName, long packageSize, string packageId)
        {
            var dialog = new Wpf.Ui.Controls.ContentDialog
            {
                Title = "Install Driver from Server?",
                Content = $"Server \"{serverName}\" wants to install driver:\n" +
                          $"  {driverName}\n" +
                          $"  Package size: {FormatBytes(packageSize)}\n" +
                          $"  Verified: ✓ (SHA-256 + size match)\n\n" +
                          $"This driver is required to print to \"{printerName}\".\n" +
                          $"The driver will be installed with administrator privileges.",
                PrimaryButtonText = "Install Driver",
                CloseButtonText = "Cancel",
                DefaultButton = Wpf.Ui.Controls.ContentDialogButton.Primary,
            };

            try
            {
                var result = await _contentDialogService.ShowAsync(dialog, CancellationToken.None);
                return result == Wpf.Ui.Controls.ContentDialogResult.Primary;
            }
            catch (Exception ex)
            {
                AppLogger.Error("[CLIENT] Driver provisioning confirmation dialog failed: " + ex.Message);
                return false;
            }
        }

        private static string FormatBytes(long bytes)
        {
            string[] sizes = ["B", "KB", "MB", "GB"];
            double len = bytes;
            int order = 0;
            while (len >= 1024 && order < sizes.Length - 1)
            {
                order++;
                len /= 1024;
            }
            return $"{len:0.##} {sizes[order]}";
        }

        /// <summary>
        /// Resolves the server-advertised driver name to a driver installed on THIS machine.
        /// Priority (issue #21): exact match → fuzzy match → driver picker UI → confirmed Generic/Text fallback.
        /// Returns null when the user cancels (installation aborted).
        /// </summary>
        private async Task<string?> ResolveDriverNameAsync(string? serverDriverName)
        {
            try
            {
                // Enumerate drivers installed on this machine.
                var localDrivers = await Task.Run(() => VirtualPrinterManager.GetInstalledDrivers());
                if (localDrivers.Count == 0)
                {
                    AppLogger.Log("[CLIENT] No locally installed drivers enumerated — falling back to server hint.");
                }

                string hint = !string.IsNullOrWhiteSpace(serverDriverName) ? serverDriverName.Trim() : "Generic / Text Only";

                // 1. Exact / fuzzy match against local drivers.
                string? resolved = DriverNameResolver.Resolve(hint, localDrivers);
                if (resolved != null)
                {
                    if (DriverNameResolver.IsDifferentResolvedName(hint, resolved))
                    {
                        AppLogger.Log($"[CLIENT] Driver resolved: server hint '{hint}' → local driver '{resolved}'.");
                        _snackbarService.Show(
                            "Driver resolved",
                            $"Server driver '{hint}' mapped to local driver '{resolved}'.",
                            ControlAppearance.Info,
                            new Wpf.Ui.Controls.SymbolIcon(Wpf.Ui.Controls.SymbolRegular.Info24),
                            TimeSpan.FromSeconds(4));
                    }
                    return resolved;
                }

                // 2. No confident match — show the driver picker (list of locally installed drivers).
                if (localDrivers.Count > 0)
                {
                    var picked = await ShowDriverPickerAsync(hint, localDrivers);
                    if (picked != null) return picked;
                    return null; // user cancelled the picker
                }

                // 3. No local drivers at all — Generic/Text fallback only with explicit confirmation.
                return await ConfirmGenericFallbackAsync(hint);
            }
            catch (Exception ex)
            {
                AppLogger.Error("[CLIENT] Driver resolution failed: " + ex.Message);
                // Fail open to the old behavior (server hint) so the user still sees the real error.
                return !string.IsNullOrWhiteSpace(serverDriverName) ? serverDriverName.Trim() : "Generic / Text Only";
            }
        }

        /// <summary>
        /// Shows a simple modal driver picker built on a ContentDialog. Returns the chosen
        /// driver name, or null if the user cancels.
        /// </summary>
        private async Task<string?> ShowDriverPickerAsync(string hint, List<string> localDrivers)
        {
            // Present up to a reasonable number of options, alphabetically sorted.
            const int maxOptions = 200;
            var all = localDrivers
                .Where(d => !string.IsNullOrWhiteSpace(d))
                .OrderBy(d => d, StringComparer.OrdinalIgnoreCase)
                .ToList();
            var options = all.Take(maxOptions).ToList();
            int hidden = all.Count - options.Count;

            if (options.Count == 0) return null;

            var picker = new Wpf.Ui.Controls.ContentDialog
            {
                Title = "Select Printer Driver",
                Content = $"The driver '{hint}' advertised by the server was not found on this computer.\n\n" +
                          "Select a locally installed driver to use for the virtual printer:",
                PrimaryButtonText = "OK",
                CloseButtonText = "Cancel",
                DefaultButton = Wpf.Ui.Controls.ContentDialogButton.Primary,
            };

            var combo = new System.Windows.Controls.ComboBox
            {
                ItemsSource = options,
                SelectedIndex = 0,
                Margin = new System.Windows.Thickness(0, 8, 0, 0),
                MinWidth = 320,
                IsTextSearchEnabled = false,
            };

            // Live search box: filters the combo items by substring (case-insensitive).
            // The underlying list is untouched — clearing the search restores all options.
            var searchBox = new System.Windows.Controls.TextBox
            {
                Margin = new System.Windows.Thickness(0, 8, 0, 0),
                MinWidth = 320,
                ToolTip = "Type to filter drivers...",
            };
            var comboView = System.Windows.Data.CollectionViewSource.GetDefaultView(options);
            searchBox.TextChanged += (_, _) =>
            {
                string filter = searchBox.Text.Trim();
                comboView.Filter = string.IsNullOrEmpty(filter)
                    ? null
                    : obj => obj is string s && DriverNameResolver.MatchesFilter(s, filter);
                // Reset selection to first visible item so clicking OK always yields a valid choice.
                comboView.MoveCurrentToFirst();
                combo.SelectedIndex = combo.Items.Count > 0 ? 0 : -1;
            };

            var message = new System.Windows.Controls.TextBlock
            {
                Text = (string)picker.Content,
                TextWrapping = System.Windows.TextWrapping.Wrap
            };
            string caption = hidden > 0
                ? $"{options.Count} installed drivers shown ({(hidden)} more not listed — use Print Management for the full list)."
                : $"{options.Count} installed driver(s) available.";

            picker.Content = new System.Windows.Controls.StackPanel
            {
                Children = { message, searchBox, combo, new System.Windows.Controls.TextBlock { Text = caption, Margin = new System.Windows.Thickness(0, 8, 0, 0), Foreground = System.Windows.Media.Brushes.Gray, FontSize = 11 } }
            };

            try
            {
                var result = await _contentDialogService.ShowAsync(picker, CancellationToken.None);
                if (result == Wpf.Ui.Controls.ContentDialogResult.Primary && combo.SelectedItem is string chosen)
                {
                    AppLogger.Log($"[CLIENT] User selected driver '{chosen}' from picker.");
                    return chosen;
                }
            }
            catch (Exception ex)
            {
                AppLogger.Error("[CLIENT] Driver picker dialog failed: " + ex.Message);
            }
            return null;
        }

        /// <summary>
        /// Confirms with the user before falling back to Generic / Text Only (never automatic).
        /// </summary>
        private async Task<string?> ConfirmGenericFallbackAsync(string hint)
        {
            var dialog = new Wpf.Ui.Controls.ContentDialog
            {
                Title = "Use Generic / Text Only?",
                Content = $"No matching printer driver was found on this computer for '{hint}'.\n\n" +
                          "Continuing with 'Generic / Text Only' may reduce print fidelity (colors, margins, quality) " +
                          "and the printer may not be detected correctly. It is recommended to install the printer's " +
                          "official driver first (connect the printer or run the manufacturer's installer).\n\n" +
                          "Continue with Generic / Text Only?",
                PrimaryButtonText = "Use Generic / Text Only",
                CloseButtonText = "Cancel",
            };

            try
            {
                var result = await _contentDialogService.ShowAsync(dialog, CancellationToken.None);
                if (result == Wpf.Ui.Controls.ContentDialogResult.Primary)
                {
                    AppLogger.Log("[CLIENT] User confirmed Generic / Text Only fallback.");
                    return "Generic / Text Only";
                }
            }
            catch (Exception ex)
            {
                AppLogger.Error("[CLIENT] Generic fallback confirmation dialog failed: " + ex.Message);
            }
            return null;
        }

        [RelayCommand]
        private async Task DeleteSelectedAsync()
        {
            if (SelectedPrinter == null || !SelectedPrinter.IsInstalled) return;
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
                    var localPrinters = SpoolerApi.GetLocalPrinters();
                    if (localPrinters.Contains(oldName, StringComparer.OrdinalIgnoreCase))
                    {
                        virtualPrinterName = oldName;
                    }
                }
            }

            StatusText = "Deleting...";

            string pipeName = config?.PipeName ?? string.Empty;
            var result = await VirtualPrinterManager.RemovePrinterAsync(virtualPrinterName, pipeName);

            if (result.Success)
            {
                if (config != null)
                {
                    _installedPrinters.Remove(config);
                    SaveConfiguration();

                    var listener = _activeListeners.FirstOrDefault(l => l.PipeName == config.PipeName);
                    if (listener != null)
                    {
                        listener.OnServerUnreachable -= TriggerTrackerRescan;
                        listener.Stop();
                        _activeListeners.Remove(listener);
                    }
                }

                item.IsInstalled = false;

                StatusText = "Deleted successfully.";
                _snackbarService.Show("Printer Removed", $"'{virtualPrinterName}' has been removed.", ControlAppearance.Info, new Wpf.Ui.Controls.SymbolIcon(Wpf.Ui.Controls.SymbolRegular.Delete24), TimeSpan.FromSeconds(3));
            }
            else
            {
                StatusText = "Deletion failed.";
                System.Windows.MessageBox.Show($"Failed to delete printer. Please run as Administrator.\n\nDetails: {result.ErrorMessage}", "Error", System.Windows.MessageBoxButton.OK, System.Windows.MessageBoxImage.Error);
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
                        if (string.IsNullOrEmpty(config.PipeName) || string.IsNullOrEmpty(config.ServerIp))
                        {
                            AppLogger.Log($"[CLIENT] Skipping invalid config entry: {config.VirtualPrinterName}");
                            continue;
                        }

                        var listener = new PipeListener(config.PipeName, config.ServerIp, config.TargetPrinterName, config.VirtualPrinterName);
                        listener.Start();
                        _activeListeners.Add(listener);
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

        [RelayCommand]
        private void CancelDownload()
        {
            _downloadCts?.Cancel();
        }

        public void StopClient()
        {
            _downloadCts?.Dispose();
            _downloadCts = null;
            foreach (var listener in _activeListeners)
            {
                listener.Stop();
            }
            Application.Current.Dispatcher.Invoke(() => Logs.Clear());
        }

        public void Dispose()
        {
            AppLogger.OnLog -= AppLogger_OnLog;
            _downloadCts?.Dispose();
            _downloadCts = null;
            foreach (var listener in _activeListeners)
            {
                listener.Stop();
            }
        }
    }
}
