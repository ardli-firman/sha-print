using System;
using System.Collections.Generic;
using System.Drawing;
using System.IO;
using System.Linq;
using System.Text.Json;
using System.Windows.Forms;
using ShaPrint.Core.Network;

namespace ShaPrint.Client
{
    public class ClientForm : Form
    {
        private Button btnScan;
        private ListBox lbServers;
        private Button btnInstall;
        private Button btnDelete;
        private Label lblStatus;
        private Label lblIp;
        private TextBox txtServerIp;
        
        private NotifyIcon trayIcon;
        private ContextMenuStrip trayMenu;

        private DiscoveryClient _discoveryClient;
        private List<DiscoveryResponseMessage> _discoveredServers = new List<DiscoveryResponseMessage>();
        private List<PipeListener> _activeListeners = new List<PipeListener>();
        
        private readonly string _configFile = "ClientConfig.json";
        private List<InstalledPrinterConfig> _installedPrinters = new List<InstalledPrinterConfig>();

        public ClientForm()
        {
            _discoveryClient = new DiscoveryClient();
            InitializeComponent();
            LoadConfiguration();
        }

        private void InitializeComponent()
        {
            this.Text = "ShaPrint Client";
            this.Size = new Size(500, 400);
            this.StartPosition = FormStartPosition.CenterScreen;

            btnScan = new Button();
            btnScan.Text = "Scan LAN for Printers";
            btnScan.Location = new Point(10, 10);
            btnScan.Size = new Size(150, 30);
            btnScan.Click += BtnScan_Click;
            this.Controls.Add(btnScan);

            lblIp = new Label();
            lblIp.Text = "Specific IP (Optional):";
            lblIp.Location = new Point(170, 15);
            lblIp.AutoSize = true;
            this.Controls.Add(lblIp);

            txtServerIp = new TextBox();
            txtServerIp.Location = new Point(295, 12);
            txtServerIp.Size = new Size(130, 20);
            this.Controls.Add(txtServerIp);

            lbServers = new ListBox();
            lbServers.Location = new Point(10, 50);
            lbServers.Size = new Size(460, 250);
            this.Controls.Add(lbServers);

            btnInstall = new Button();
            btnInstall.Text = "Install Selected Printer";
            btnInstall.Location = new Point(10, 310);
            btnInstall.Size = new Size(150, 30);
            btnInstall.Enabled = false;
            btnInstall.Click += BtnInstall_Click;
            this.Controls.Add(btnInstall);

            btnDelete = new Button();
            btnDelete.Text = "Delete Selected Printer";
            btnDelete.Location = new Point(170, 310);
            btnDelete.Size = new Size(150, 30);
            btnDelete.Enabled = false;
            btnDelete.Click += BtnDelete_Click;
            this.Controls.Add(btnDelete);

            lblStatus = new Label();
            lblStatus.Text = "Ready";
            lblStatus.Location = new Point(10, 345);
            lblStatus.AutoSize = true;
            this.Controls.Add(lblStatus);

            lbServers.SelectedIndexChanged += (s, e) => 
            { 
                btnInstall.Enabled = lbServers.SelectedIndex >= 0; 
                btnDelete.Enabled = lbServers.SelectedIndex >= 0; 
            };

            trayMenu = new ContextMenuStrip();
            trayMenu.Items.Add("Open", null, (s, e) => { this.Show(); this.WindowState = FormWindowState.Normal; });
            trayMenu.Items.Add("Exit", null, (s, e) => { Application.Exit(); });

            trayIcon = new NotifyIcon();
            trayIcon.Text = "ShaPrint Client";
            trayIcon.Icon = SystemIcons.Application;
            trayIcon.ContextMenuStrip = trayMenu;
            trayIcon.Visible = true;
            trayIcon.DoubleClick += (s, e) => { this.Show(); this.WindowState = FormWindowState.Normal; };
        }

        private async void BtnScan_Click(object? sender, EventArgs e)
        {
            string? targetIp = null;
            if (!string.IsNullOrWhiteSpace(txtServerIp.Text))
            {
                if (!System.Net.IPAddress.TryParse(txtServerIp.Text.Trim(), out _))
                {
                    MessageBox.Show("Format IP Address tidak valid!", "Error", MessageBoxButtons.OK, MessageBoxIcon.Error);
                    return;
                }
                targetIp = txtServerIp.Text.Trim();
            }

            btnScan.Enabled = false;
            lblStatus.Text = "Scanning...";
            lbServers.Items.Clear();

            _discoveredServers = await _discoveryClient.DiscoverServersAsync(targetIp);

            foreach (var server in _discoveredServers)
            {
                foreach (var printer in server.ExposedPrinters)
                {
                    string virtualPrinterName = $"ShaPrint - {printer.Name}";
                    bool isInstalled = _installedPrinters.Any(p => p.VirtualPrinterName == virtualPrinterName);
                    
                    lbServers.Items.Add(new PrinterDisplayItem 
                    { 
                        Server = server, 
                        Printer = printer,
                        IsInstalled = isInstalled
                    });
                }
            }

            lblStatus.Text = $"Found {_discoveredServers.Count} server(s).";
            btnScan.Enabled = true;
        }

        private async void BtnInstall_Click(object? sender, EventArgs e)
        {
            if (lbServers.SelectedItem is PrinterDisplayItem item)
            {
                string virtualPrinterName = $"ShaPrint - {item.Printer.Name}";

                if (_installedPrinters.Any(p => p.VirtualPrinterName == virtualPrinterName))
                {
                    MessageBox.Show("This printer is already installed!", "Info", MessageBoxButtons.OK, MessageBoxIcon.Information);
                    return;
                }

                btnInstall.Enabled = false;
                lblStatus.Text = "Installing...";

                string safeName = item.Printer.Name.Replace(" ", "_").Replace("\\", "_");
                string pipeName = $@"\\.\pipe\shaprint_{item.Server.ServerName}_{safeName}";

                var result = await VirtualPrinterManager.InstallPrinterAsync(virtualPrinterName, pipeName);

                if (result.Success)
                {
                    var listener = new PipeListener(pipeName, item.Server.IpAddress, item.Printer.Name);
                    listener.Start();
                    _activeListeners.Add(listener);

                    _installedPrinters.Add(new InstalledPrinterConfig
                    {
                        VirtualPrinterName = virtualPrinterName,
                        PipeName = pipeName,
                        ServerIp = item.Server.IpAddress,
                        TargetPrinterName = item.Printer.Name
                    });
                    SaveConfiguration();

                    item.IsInstalled = true;
                    lbServers.Items[lbServers.SelectedIndex] = item; // Refresh display

                    lblStatus.Text = "Installed successfully! You can now print to it.";
                    MessageBox.Show($"Printer '{virtualPrinterName}' has been installed.\n\nYou can now select it when printing from Word, Chrome, etc.", "Success", MessageBoxButtons.OK, MessageBoxIcon.Information);
                }
                else
                {
                    lblStatus.Text = "Installation failed.";
                    MessageBox.Show($"Failed to install printer. Please ensure you run this application as Administrator.\n\nDetails: {result.ErrorMessage}", "Error", MessageBoxButtons.OK, MessageBoxIcon.Error);
                }

                btnInstall.Enabled = true;
            }
        }

        private async void BtnDelete_Click(object? sender, EventArgs e)
        {
            if (lbServers.SelectedItem is PrinterDisplayItem item)
            {
                string virtualPrinterName = $"ShaPrint - {item.Printer.Name}";
                var config = _installedPrinters.FirstOrDefault(p => p.VirtualPrinterName == virtualPrinterName);

                if (config == null)
                {
                    MessageBox.Show("This printer is not installed yet.", "Info", MessageBoxButtons.OK, MessageBoxIcon.Information);
                    return;
                }

                btnDelete.Enabled = false;
                lblStatus.Text = "Deleting...";

                bool removed = await VirtualPrinterManager.RemovePrinterAsync(config.VirtualPrinterName, config.PipeName);

                if (removed)
                {
                    _installedPrinters.Remove(config);
                    SaveConfiguration();

                    item.IsInstalled = false;
                    lbServers.Items[lbServers.SelectedIndex] = item; // Refresh display

                    // Stop the listener
                    var listener = _activeListeners.FirstOrDefault(l => l.PipeName == config.PipeName);
                    if (listener != null)
                    {
                        listener.Stop();
                        _activeListeners.Remove(listener);
                    }

                    lblStatus.Text = "Deleted successfully.";
                    MessageBox.Show($"Printer '{virtualPrinterName}' has been removed.", "Success", MessageBoxButtons.OK, MessageBoxIcon.Information);
                }
                else
                {
                    lblStatus.Text = "Deletion failed.";
                    MessageBox.Show("Failed to delete printer. Please run as Administrator.", "Error", MessageBoxButtons.OK, MessageBoxIcon.Error);
                }

                btnDelete.Enabled = true;
            }
        }

        protected override void OnFormClosing(FormClosingEventArgs e)
        {
            if (e.CloseReason == CloseReason.UserClosing)
            {
                e.Cancel = true;
                this.Hide();
                trayIcon.ShowBalloonTip(2000, "ShaPrint Client", "Client is running in the background to catch print jobs.", ToolTipIcon.Info);
            }
            else
            {
                foreach (var listener in _activeListeners)
                {
                    listener.Stop();
                }
                trayIcon.Visible = false;
            }
        }

        private void LoadConfiguration()
        {
            if (File.Exists(_configFile))
            {
                try
                {
                    string json = File.ReadAllText(_configFile);
                    var saved = JsonSerializer.Deserialize<List<InstalledPrinterConfig>>(json);
                    if (saved != null)
                    {
                        _installedPrinters = saved;
                        foreach (var config in _installedPrinters)
                        {
                            var listener = new PipeListener(config.PipeName, config.ServerIp, config.TargetPrinterName);
                            listener.Start();
                            _activeListeners.Add(listener);
                        }
                    }
                }
                catch { }
            }
        }

        private void SaveConfiguration()
        {
            try
            {
                string json = JsonSerializer.Serialize(_installedPrinters);
                File.WriteAllText(_configFile, json);
            }
            catch { }
        }

        private class PrinterDisplayItem
        {
            public DiscoveryResponseMessage Server { get; set; } = null!;
            public PrinterInfo Printer { get; set; } = null!;
            public bool IsInstalled { get; set; } = false;
            public override string ToString() => $"[{Server.ServerName}] {Printer.Name} {(IsInstalled ? "(INSTALLED)" : "")}";
        }

        public class InstalledPrinterConfig
        {
            public string VirtualPrinterName { get; set; } = string.Empty;
            public string PipeName { get; set; } = string.Empty;
            public string ServerIp { get; set; } = string.Empty;
            public string TargetPrinterName { get; set; } = string.Empty;
        }
    }
}
