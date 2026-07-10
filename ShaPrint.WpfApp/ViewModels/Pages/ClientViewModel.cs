using CommunityToolkit.Mvvm.ComponentModel;
using CommunityToolkit.Mvvm.Input;
using ShaPrint.Core;
using ShaPrint.Core.Network;
using ShaPrint.Client;
using ShaPrint.WpfApp.Views.Pages;
using System;
using System.Collections.Generic;
using System.Collections.ObjectModel;
using System.IO;
using System.Linq;
using System.Text.Json;
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
        private readonly string _configFile;

        private List<InstalledPrinterConfig> _installedPrinters = new();
        private List<PipeListener> _activeListeners = new();
        private ServerReachabilityTracker? _tracker;

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

        public ClientViewModel(INavigationService navigationService, ISnackbarService snackbarService)
        {
            _navigationService = navigationService;
            _snackbarService = snackbarService;
            _discoveryClient = new DiscoveryClient();
            
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
            // Fire-and-forget. Tracker handles its own debounce + cancellation.
            _ = _tracker?.RequestRescanAsync(ServerReachabilityTracker.RescanReason.PrintFailed, CancellationToken.None);
        }

        private void RestartListener(InstalledPrinterConfig cfg)
        {
            var oldListener = _activeListeners.FirstOrDefault(l => l.PipeName == cfg.PipeName);
            if (oldListener != null)
            {
                oldListener.OnServerUnreachable -= TriggerTrackerRescan;
                oldListener.Stop();
            }

            // 200ms grace for the OS to release the named pipe handle.
            Thread.Sleep(200);

            var fresh = new PipeListener(cfg.PipeName, cfg.ServerIp, cfg.TargetPrinterName, cfg.VirtualPrinterName);
            fresh.OnServerUnreachable += TriggerTrackerRescan;
            fresh.Start();

            if (oldListener != null)
            {
                int idx = _activeListeners.IndexOf(oldListener);
                _activeListeners[idx] = fresh;
            }
            else
            {
                _activeListeners.Add(fresh);
            }
        }

        private void OnServerIdentityChanged(ServerIdentityChangedArgs args)
        {
            var cfg = _installedPrinters.FirstOrDefault(c => c.PipeName == args.PipeName);
            if (cfg != null) RestartListener(cfg);

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

            var localPrinters = ShaPrint.Server.SpoolerApi.GetLocalPrinters();
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
                string driverName = Validators.ValidateDriverName(
                    !string.IsNullOrEmpty(item.Printer.DriverName) ? item.Printer.DriverName : "Generic / Text Only");
                string serverIp = Validators.ValidateIpAddress(item.Server.IpAddress);

                string virtualPrinterName = $"ShaPrint [{serverName}] - {printerName}";

                if (_installedPrinters.Any(p => p.VirtualPrinterName == virtualPrinterName))
                {
                    System.Windows.MessageBox.Show("This printer is already installed!", "Info", System.Windows.MessageBoxButton.OK, System.Windows.MessageBoxImage.Information);
                    return;
                }

                StatusText = "Installing...";

                string pipeName = $@"\\.\pipe\shaprint_{Guid.NewGuid():N}";
                AppLogger.Log($"[CLIENT] Installing virtual printer '{virtualPrinterName}' with pipe '{pipeName}'...");

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
                    var localPrinters = ShaPrint.Server.SpoolerApi.GetLocalPrinters();
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
                            await Task.Delay(2000);
                            if (_tracker == null) InitializeTracker();
                            if (_tracker != null)
                            {
                                await _tracker.RequestRescanAsync(ServerReachabilityTracker.RescanReason.Startup, CancellationToken.None);
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

        public void StopClient()
        {
            foreach (var listener in _activeListeners)
            {
                listener.Stop();
            }
            Application.Current.Dispatcher.Invoke(() => Logs.Clear());
        }

        public void Dispose()
        {
            AppLogger.OnLog -= AppLogger_OnLog;
            foreach (var listener in _activeListeners)
            {
                listener.Stop();
            }
        }
    }
}
