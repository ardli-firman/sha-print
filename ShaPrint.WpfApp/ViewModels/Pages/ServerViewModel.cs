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

    public partial class ServerViewModel : ObservableObject, IDisposable
    {
        private readonly DiscoveryServer _discoveryServer;
        private readonly PrintReceiver _printReceiver;
        private readonly ShaPrint.WpfApp.Services.Server.PrintMonitorService _printMonitorService;
        private readonly INavigationService _navigationService;
        private readonly ISnackbarService _snackbarService;
        private readonly string _configFile;

        [ObservableProperty]
        private bool _isRunning;

        [ObservableProperty]
        private string _statusText = "Status: Stopped";

        public ObservableCollection<PrinterItem> Printers { get; } = new();
        public ObservableCollection<string> Logs { get; } = new();

        public ServerViewModel(INavigationService navigationService, ISnackbarService snackbarService, ShaPrint.WpfApp.Services.Server.PrintMonitorService printMonitorService)
        {
            _navigationService = navigationService;
            _snackbarService = snackbarService;
            _printMonitorService = printMonitorService;
            _discoveryServer = new DiscoveryServer();
            _printReceiver = new PrintReceiver();
            
            string dir = Path.Combine(Environment.GetFolderPath(Environment.SpecialFolder.LocalApplicationData), "ShaPrint");
            if (!Directory.Exists(dir)) Directory.CreateDirectory(dir);
            _configFile = Path.Combine(dir, "ServerConfig.json");

            AppLogger.OnLog += AppLogger_OnLog;

            LoadPrinters();
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

            if (selectedPrinters.Count == 0)
            {
                System.Windows.MessageBox.Show("Please select at least one printer to expose.", "Warning", System.Windows.MessageBoxButton.OK, System.Windows.MessageBoxImage.Warning);
                return;
            }

            _discoveryServer.SetExposedPrinters(selectedPrinters);
            _discoveryServer.Start();
            _printReceiver.Start();
            _printMonitorService.Start();

            // Ensure firewall rules are applied and logged whenever server starts
            FirewallManager.CheckAndAddFirewallRules();

            IsRunning = true;
            StatusText = "Status: Running";

            SaveConfiguration(selectedPrinters);
            _snackbarService.Show("Server Started", $"Broadcasting {selectedPrinters.Count} printers to the network.", ControlAppearance.Success, new Wpf.Ui.Controls.SymbolIcon(Wpf.Ui.Controls.SymbolRegular.Play24), TimeSpan.FromSeconds(3));
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

                var savedPrinters = JsonSerializer.Deserialize<List<string>>(raw);
                if (savedPrinters != null && savedPrinters.Count > 0)
                {
                    foreach (var p in Printers)
                    {
                        if (savedPrinters.Contains(p.Name))
                        {
                            p.IsSelected = true;
                        }
                    }
                    StartServer();
                }
            }
            catch (Exception ex) { AppLogger.Error("Failed to load server configuration", ex); }
        }

        private void SaveConfiguration(List<string> printers)
        {
            try
            {
                string json = JsonSerializer.Serialize(printers);
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
