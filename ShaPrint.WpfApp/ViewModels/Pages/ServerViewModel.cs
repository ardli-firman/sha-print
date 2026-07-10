using CommunityToolkit.Mvvm.ComponentModel;
using CommunityToolkit.Mvvm.Input;
using ShaPrint.Core;
using ShaPrint.Server;
using System;
using System.Collections.Generic;
using System.Collections.ObjectModel;
using System.IO;
using System.Linq;
using System.Text.Json;
using System.Windows;
using Wpf.Ui;
using Wpf.Ui.Controls;
using ShaPrint.WpfApp.Views.Pages;
using ShaPrint.WpfApp.Services;
using System.Collections.Concurrent;
using ShaPrint.Core.Network;
using ShaPrint.WpfApp.Services.Server;

namespace ShaPrint.WpfApp.ViewModels.Pages
{
    public partial class PrinterItem : ObservableObject
    {
        [ObservableProperty]
        private string _name;

        [ObservableProperty]
        private bool _isSelected;

        public PrinterItem(string name)
        {
            _name = name;
        }
    }

    public partial class ScannerItem : ObservableObject
    {
        [ObservableProperty]
        private string _name;

        [ObservableProperty]
        private bool _isSelected;

        public ScannerItem(string name)
        {
            _name = name;
        }
    }

    public class ServerSavedConfig
    {
        public List<string> ExposedPrinters { get; set; } = new();
        public List<string> ExposedScanners { get; set; } = new();

        /// <summary>
        /// Stable server identity UUID. Generated once on first save, persisted forever.
        /// Null for servers that pre-date this feature. Broadcast in discovery responses.
        /// </summary>
        public string? ServerId { get; set; }
    }

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

        private readonly DiscoveryServer _discoveryServer;
        private readonly PrintReceiver _printReceiver;
        private readonly ShaPrint.WpfApp.Services.Server.PrintMonitorService _printMonitorService;
        private readonly ScannerService _scannerService;
        private readonly INavigationService _navigationService;
        private readonly ISnackbarService _snackbarService;
        private readonly string _configFile;
        private MonitorTcpServer? _monitorTcpServer;

        public DateTime? ServerStartTime { get; private set; }
        public ConcurrentQueue<JobHistoryEntry> RecentJobs { get; } = new();
        public ConcurrentQueue<ServerErrorEntry> Errors { get; } = new();
        public List<string> ExposedPrinters { get; private set; } = new();
        public List<string> ExposedScanners { get; private set; } = new();

        public DiscoveryServer DiscoveryServer => _discoveryServer;

        /// <summary>
        /// Stable server identity. Null until the first <see cref="SaveConfiguration"/> call.
        /// </summary>
        public string? ServerId { get; private set; }

        [ObservableProperty]
        private bool _isRunning;

        [ObservableProperty]
        private string _statusText = "Status: Stopped";

        public ObservableCollection<PrinterItem> Printers { get; } = new();
        public ObservableCollection<ScannerItem> Scanners { get; } = new();
        public ObservableCollection<string> Logs { get; } = new();
        public string LogsText => string.Join(Environment.NewLine, Logs);

        public ServerViewModel(INavigationService navigationService, ISnackbarService snackbarService, ShaPrint.WpfApp.Services.Server.PrintMonitorService printMonitorService, INotificationService notificationService)
        {
            _navigationService = navigationService;
            _snackbarService = snackbarService;
            _printMonitorService = printMonitorService;
            _scannerService = new ScannerService();
            _discoveryServer = new DiscoveryServer(notificationService);
            _printReceiver = new PrintReceiver(notificationService, LogJob, LogError);
            
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

            if (Application.Current?.Dispatcher != null)
            {
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
            else
            {
                Logs.Insert(0, $"[{DateTime.Now:HH:mm:ss}] {msg}");
                if (Logs.Count > 200)
                {
                    Logs.RemoveAt(Logs.Count - 1);
                }
                OnPropertyChanged(nameof(LogsText));
            }
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
            OnPropertyChanged(nameof(ToggleButtonAppearance));
        }

        public string ToggleButtonText => IsRunning ? "Stop Server" : "Start Server";
        public bool IsNotRunning => !IsRunning;
        public Wpf.Ui.Controls.ControlAppearance ToggleButtonAppearance => IsRunning ? Wpf.Ui.Controls.ControlAppearance.Danger : Wpf.Ui.Controls.ControlAppearance.Primary;

        private void LoadPrinters()
        {
            if (IsUnitTest) return;
            var printers = SpoolerApi.GetLocalPrinters();
            Printers.Clear();
            foreach (var p in printers)
            {
                Printers.Add(new PrinterItem(p));
            }
        }

        private void LoadScanners()
        {
            if (IsUnitTest) return;
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
                StartServer();
            }
        }

        private void StartServer()
        {
            List<string> selectedPrinters;
            List<string> selectedScanners;

            if (IsUnitTest)
            {
                selectedPrinters = ExposedPrinters;
                selectedScanners = ExposedScanners;
            }
            else
            {
                selectedPrinters = Printers.Where(p => p.IsSelected).Select(p => p.Name).ToList();
                selectedScanners = Scanners.Where(s => s.IsSelected).Select(s => s.Name).ToList();

                if (selectedPrinters.Count == 0 && selectedScanners.Count == 0)
                {
                    System.Windows.MessageBox.Show("Please select at least one printer or scanner to expose.", "Warning", System.Windows.MessageBoxButton.OK, System.Windows.MessageBoxImage.Warning);
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
            _printMonitorService?.SetMonitoredPrinters(selectedPrinters);
            _discoveryServer.Start();
            _printReceiver.Start();
            _printMonitorService?.Start();

            // Start Monitor TCP Server
            _monitorTcpServer = new MonitorTcpServer(new ServerStatusProvider(this));
            _monitorTcpServer.Start();

            // Ensure firewall rules are applied and logged whenever server starts
            if (!IsUnitTest)
            {
                FirewallManager.CheckAndAddFirewallRules();
            }

            IsRunning = true;
            StatusText = "Status: Running";

            SaveConfiguration(selectedPrinters, selectedScanners);
            _snackbarService?.Show("Server Started", $"Broadcasting {selectedPrinters.Count} printers and {selectedScanners.Count} scanners.", ControlAppearance.Success, new Wpf.Ui.Controls.SymbolIcon(Wpf.Ui.Controls.SymbolRegular.Play24), TimeSpan.FromSeconds(3));
        }

        public void StopServer()
        {
            if (!IsRunning) return;

            _discoveryServer.Stop();
            _printReceiver.Stop();
            _printMonitorService?.Stop();

            if (_monitorTcpServer != null)
            {
                _monitorTcpServer.Stop();
                _monitorTcpServer = null;
            }

            ServerStartTime = null;
            ExposedPrinters.Clear();
            ExposedScanners.Clear();

            IsRunning = false;
            StatusText = "Status: Stopped";
            
            if (Application.Current != null)
            {
                Application.Current.Dispatcher.Invoke(() => Logs.Clear());
            }
            else
            {
                Logs.Clear();
            }
            _snackbarService?.Show("Server Stopped", "Server has been stopped.", ControlAppearance.Info, new Wpf.Ui.Controls.SymbolIcon(Wpf.Ui.Controls.SymbolRegular.Stop24), TimeSpan.FromSeconds(3));
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
                    }
                }
                catch
                {
                    // Fallback to legacy List<string> for printers
                    savedPrinters = JsonSerializer.Deserialize<List<string>>(raw);
                }

                if (savedPrinters != null)
                {
                    foreach (var p in Printers)
                    {
                        if (savedPrinters.Contains(p.Name))
                        {
                            p.IsSelected = true;
                        }
                    }
                }

                if (savedScanners != null)
                {
                    foreach (var s in Scanners)
                    {
                        if (savedScanners.Contains(s.Name))
                        {
                            s.IsSelected = true;
                        }
                    }
                }

                if ((savedPrinters != null && savedPrinters.Count > 0) || (savedScanners != null && savedScanners.Count > 0))
                {
                    StartServer();
                }
            }
            catch (Exception ex) { AppLogger.Error("Failed to load server configuration", ex); }
        }

        private void SaveConfiguration(List<string> printers, List<string> scanners)
        {
            if (IsUnitTest) return;
            try
            {
                if (string.IsNullOrEmpty(ServerId))
                {
                    ServerId = Guid.NewGuid().ToString("N");
                }

                var config = new ServerSavedConfig
                {
                    ExposedPrinters = printers,
                    ExposedScanners = scanners,
                    ServerId = ServerId
                };
                string json = JsonSerializer.Serialize(config);
                string wrapped = CryptoHelper.WrapConfigWithHmac(json);
                File.WriteAllText(_configFile, wrapped);
            }
            catch (Exception ex) { AppLogger.Error("Failed to save server configuration", ex); }
        }

        public void Dispose()
        {
            AppLogger.OnLog -= AppLogger_OnLog;
            if (IsRunning) StopServer();
        }
    }
}
