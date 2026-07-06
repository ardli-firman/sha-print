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
    }

    public partial class ServerViewModel : ObservableObject, IDisposable
    {
        private readonly DiscoveryServer _discoveryServer;
        private readonly PrintReceiver _printReceiver;
        private readonly ShaPrint.WpfApp.Services.Server.PrintMonitorService _printMonitorService;
        private readonly ScannerService _scannerService;
        private readonly INavigationService _navigationService;
        private readonly ISnackbarService _snackbarService;
        private readonly string _configFile;

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
            _printReceiver = new PrintReceiver(notificationService);
            
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

            Application.Current.Dispatcher.Invoke(() =>
            {
                Logs.Insert(0, $"[{DateTime.Now:HH:mm:ss}] {msg}");
                if (Logs.Count > 200)
                {
                    Logs.RemoveAt(Logs.Count - 1);
                }
                OnPropertyChanged(nameof(LogsText));
            });
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
            var printers = SpoolerApi.GetLocalPrinters();
            Printers.Clear();
            foreach (var p in printers)
            {
                Printers.Add(new PrinterItem(p));
            }
        }

        private void LoadScanners()
        {
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
            var selectedPrinters = Printers.Where(p => p.IsSelected).Select(p => p.Name).ToList();
            var selectedScanners = Scanners.Where(s => s.IsSelected).Select(s => s.Name).ToList();

            if (selectedPrinters.Count == 0 && selectedScanners.Count == 0)
            {
                System.Windows.MessageBox.Show("Please select at least one printer or scanner to expose.", "Warning", System.Windows.MessageBoxButton.OK, System.Windows.MessageBoxImage.Warning);
                return;
            }

            _discoveryServer.SetExposedPrinters(selectedPrinters);
            _discoveryServer.SetExposedScanners(selectedScanners);
            _printMonitorService.SetMonitoredPrinters(selectedPrinters);
            _discoveryServer.Start();
            _printReceiver.Start();
            _printMonitorService.Start();

            // Ensure firewall rules are applied and logged whenever server starts
            FirewallManager.CheckAndAddFirewallRules();

            IsRunning = true;
            StatusText = "Status: Running";

            SaveConfiguration(selectedPrinters, selectedScanners);
            _snackbarService.Show("Server Started", $"Broadcasting {selectedPrinters.Count} printers and {selectedScanners.Count} scanners.", ControlAppearance.Success, new Wpf.Ui.Controls.SymbolIcon(Wpf.Ui.Controls.SymbolRegular.Play24), TimeSpan.FromSeconds(3));
        }

        public void StopServer()
        {
            if (!IsRunning) return;

            _discoveryServer.Stop();
            _printReceiver.Stop();
            _printMonitorService.Stop();

            IsRunning = false;
            StatusText = "Status: Stopped";
            
            Application.Current.Dispatcher.Invoke(() => Logs.Clear());
            _snackbarService.Show("Server Stopped", "Server has been stopped.", ControlAppearance.Info, new Wpf.Ui.Controls.SymbolIcon(Wpf.Ui.Controls.SymbolRegular.Stop24), TimeSpan.FromSeconds(3));
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
            try
            {
                var config = new ServerSavedConfig
                {
                    ExposedPrinters = printers,
                    ExposedScanners = scanners
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
