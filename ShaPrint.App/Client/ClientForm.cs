using System;
using System.Collections.Generic;
using System.Drawing;
using System.IO;
using System.Linq;
using System.Text.Json;
using System.Windows.Forms;
using ShaPrint.Core;
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
        private CheckBox chkRunOnStartup;

        private NotifyIcon trayIcon;
        private ContextMenuStrip trayMenu;

        private DiscoveryClient _discoveryClient;
        private List<DiscoveryResponseMessage> _discoveredServers = new List<DiscoveryResponseMessage>();
        private List<PipeListener> _activeListeners = new List<PipeListener>();

        private bool _startHidden;
        private string _configFile;

        private string GetConfigPath(string fileName)
        {
            string dir = Path.Combine(Environment.GetFolderPath(Environment.SpecialFolder.LocalApplicationData), "ShaPrint");
            if (!Directory.Exists(dir)) Directory.CreateDirectory(dir);
            return Path.Combine(dir, fileName);
        }

        private List<InstalledPrinterConfig> _installedPrinters = new List<InstalledPrinterConfig>();

        public ClientForm(bool startHidden = false)
        {
            _startHidden = startHidden;
            _configFile = GetConfigPath("ClientConfig.json");
            _discoveryClient = new DiscoveryClient();
            InitializeComponent();
            LoadConfiguration();
        }

        private void InitializeComponent()
        {
            this.Text = "ShaPrint Client";
            this.Size = new Size(520, 450);
            this.StartPosition = FormStartPosition.CenterScreen;
            try { this.Icon = Icon.ExtractAssociatedIcon(Application.ExecutablePath); } catch { }

            var grpNetwork = new GroupBox();
            grpNetwork.Text = "Network Discovery";
            grpNetwork.Location = new Point(10, 10);
            grpNetwork.Size = new Size(480, 80);
            this.Controls.Add(grpNetwork);

            var lblHint = new Label();
            lblHint.Text = "Hint: If Server is on a different Wi-Fi/VLAN, auto-scan won't work. Enter Server IP explicitly.";
            lblHint.Location = new Point(10, 20);
            lblHint.AutoSize = true;
            lblHint.ForeColor = Color.DarkSlateGray;
            grpNetwork.Controls.Add(lblHint);

            lblIp = new Label();
            lblIp.Text = "Specific Server IP :";
            lblIp.Location = new Point(10, 47);
            lblIp.AutoSize = true;
            grpNetwork.Controls.Add(lblIp);

            txtServerIp = new TextBox();
            txtServerIp.Location = new Point(125, 45);
            txtServerIp.Size = new Size(130, 20);
            grpNetwork.Controls.Add(txtServerIp);

            var toolTip = new ToolTip();
            toolTip.SetToolTip(txtServerIp, "Example: 192.168.1.50\nLeave blank for auto-discovery on the same local network.");

            btnScan = new Button();
            btnScan.Text = "Scan LAN / Connect";
            btnScan.Location = new Point(270, 43);
            btnScan.Size = new Size(150, 25);
            btnScan.Click += BtnScan_Click;
            grpNetwork.Controls.Add(btnScan);

            lbServers = new ListBox();
            lbServers.Location = new Point(10, 100);
            lbServers.Size = new Size(480, 240);
            this.Controls.Add(lbServers);

            btnInstall = new Button();
            btnInstall.Text = "Install Selected Printer";
            btnInstall.Location = new Point(10, 350);
            btnInstall.Size = new Size(150, 30);
            btnInstall.Enabled = false;
            btnInstall.Click += BtnInstall_Click;
            this.Controls.Add(btnInstall);

            btnDelete = new Button();
            btnDelete.Text = "Delete Selected Printer";
            btnDelete.Location = new Point(170, 350);
            btnDelete.Size = new Size(150, 30);
            btnDelete.Enabled = false;
            btnDelete.Click += BtnDelete_Click;
            this.Controls.Add(btnDelete);

            Button btnSwitchMode = new Button();
            btnSwitchMode.Text = "Switch to Server Mode";
            btnSwitchMode.Location = new Point(340, 350);
            btnSwitchMode.Size = new Size(150, 30);
            btnSwitchMode.Click += (s, e) => SwitchMode();
            this.Controls.Add(btnSwitchMode);

            lblStatus = new Label();
            lblStatus.Text = "Ready";
            lblStatus.Location = new Point(10, 390);
            lblStatus.AutoSize = true;
            this.Controls.Add(lblStatus);

            chkRunOnStartup = new CheckBox();
            chkRunOnStartup.Text = "Run Automatically on Windows Startup";
            chkRunOnStartup.Location = new Point(10, 420);
            chkRunOnStartup.AutoSize = true;
            chkRunOnStartup.Checked = ShaPrint.App.StartupManager.IsStartupEnabled();
            chkRunOnStartup.CheckedChanged += (s, e) => ShaPrint.App.StartupManager.SetStartup(chkRunOnStartup.Checked);
            this.Controls.Add(chkRunOnStartup);

            var txtLog = new TextBox();
            txtLog.Multiline = true;
            txtLog.ReadOnly = true;
            txtLog.ScrollBars = ScrollBars.Vertical;
            txtLog.Location = new Point(10, 445);
            txtLog.Size = new Size(480, 110);
            txtLog.BackColor = Color.Black;
            txtLog.ForeColor = Color.Lime;
            txtLog.Font = new Font("Consolas", 9F);
            this.Controls.Add(txtLog);

            this.Size = new Size(520, 600);

            ShaPrint.Core.AppLogger.OnLog += (msg) =>
            {
                if (this.IsHandleCreated)
                {
                    this.Invoke(new Action(() => {
                        txtLog.AppendText(msg + Environment.NewLine);
                    }));
                }
            };

            lbServers.SelectedIndexChanged += (s, e) =>
            {
                if (lbServers.SelectedItem is PrinterDisplayItem item)
                {
                    btnInstall.Enabled = !item.IsInstalled;
                    btnDelete.Enabled = item.IsInstalled;
                }
                else
                {
                    btnInstall.Enabled = false;
                    btnDelete.Enabled = false;
                }
            };

            trayMenu = new ContextMenuStrip();
            trayMenu.Items.Add("Open", null, (s, e) => { this.Show(); this.WindowState = FormWindowState.Normal; });
            trayMenu.Items.Add("Exit", null, (s, e) => { Application.Exit(); });

            trayIcon = new NotifyIcon();
            trayIcon.Text = "ShaPrint Client";
            try { trayIcon.Icon = Icon.ExtractAssociatedIcon(Application.ExecutablePath); } catch { trayIcon.Icon = SystemIcons.Application; }
            trayIcon.ContextMenuStrip = trayMenu;
            trayIcon.Visible = true;
            trayIcon.DoubleClick += (s, e) => { this.Show(); this.WindowState = FormWindowState.Normal; };
        }

        protected override void SetVisibleCore(bool value)
        {
            if (_startHidden)
            {
                value = false;
                if (!this.IsHandleCreated) CreateHandle();
                _startHidden = false;
            }
            base.SetVisibleCore(value);
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
            if (lbServers.SelectedItem is not PrinterDisplayItem item)
                return;

            try
            {
                // Validate ALL input from network before use
                string serverName = Validators.ValidateServerName(item.Server.ServerName);
                string printerName = Validators.ValidatePrinterName(item.Printer.Name);
                string driverName = Validators.ValidateDriverName(
                    !string.IsNullOrEmpty(item.Printer.DriverName) ? item.Printer.DriverName : "Generic / Text Only");
                string serverIp = Validators.ValidateIpAddress(item.Server.IpAddress);

                string virtualPrinterName = $"ShaPrint - {printerName}";

                if (_installedPrinters.Any(p => p.VirtualPrinterName == virtualPrinterName))
                {
                    MessageBox.Show("This printer is already installed!", "Info", MessageBoxButtons.OK, MessageBoxIcon.Information);
                    return;
                }

                btnInstall.Enabled = false;
                lblStatus.Text = "Installing...";

                // Use random GUID for pipe name — not predictable from network data
                string pipeName = $@"\\.\pipe\shaprint_{Guid.NewGuid():N}";

                AppLogger.Log($"[CLIENT] Installing virtual printer '{virtualPrinterName}' with pipe '{pipeName}'...");

                var result = await VirtualPrinterManager.InstallPrinterAsync(virtualPrinterName, pipeName, driverName);

                if (result.Success)
                {
                    var listener = new PipeListener(pipeName, serverIp, printerName);
                    listener.Start();
                    _activeListeners.Add(listener);

                    _installedPrinters.Add(new InstalledPrinterConfig
                    {
                        VirtualPrinterName = virtualPrinterName,
                        PipeName = pipeName,
                        ServerIp = serverIp,
                        TargetPrinterName = printerName
                    });
                    SaveConfiguration();

                    item.IsInstalled = true;
                    lbServers.Items[lbServers.SelectedIndex] = item;

                    lblStatus.Text = "Installed successfully! You can now print to it.";
                    MessageBox.Show($"Printer '{virtualPrinterName}' has been installed.\n\nYou can now select it when printing from Word, Chrome, etc.",
                        "Success", MessageBoxButtons.OK, MessageBoxIcon.Information);
                }
                else
                {
                    lblStatus.Text = "Installation failed.";
                    MessageBox.Show($"Failed to install printer. Please ensure you run this application as Administrator.\n\nDetails: {result.ErrorMessage}",
                        "Error", MessageBoxButtons.OK, MessageBoxIcon.Error);
                }
            }
            catch (ArgumentException ex)
            {
                lblStatus.Text = "Installation rejected.";
                MessageBox.Show($"Security: {ex.Message}\n\nThe server may be sending invalid or malicious data.",
                    "Security Warning", MessageBoxButtons.OK, MessageBoxIcon.Warning);
                AppLogger.Error($"[CLIENT] Input validation failed: {ex.Message}");
            }
            finally
            {
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
                    lbServers.Items[lbServers.SelectedIndex] = item;

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

        private bool _isSwitchingMode = false;

        private void SwitchMode()
        {
            var result = MessageBox.Show("Are you sure you want to switch to Server Mode?\n\nThis will restart the application.",
                "Switch Mode", MessageBoxButtons.YesNo, MessageBoxIcon.Question);
            if (result == DialogResult.Yes)
            {
                _isSwitchingMode = true;
                foreach (var listener in _activeListeners)
                {
                    listener.Stop();
                }
                trayIcon.Visible = false;

                try
                {
                    if (File.Exists(GetConfigPath("AppMode.json")))
                        File.Delete(GetConfigPath("AppMode.json"));
                }
                catch (Exception ex) { AppLogger.Error("Failed to delete AppMode.json", ex); }

                Application.Restart();
                Environment.Exit(0);
            }
        }

        protected override void OnFormClosing(FormClosingEventArgs e)
        {
            if (e.CloseReason == CloseReason.UserClosing && !_isSwitchingMode)
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
                    string raw = File.ReadAllText(_configFile);
                    string? json = CryptoHelper.UnwrapConfigWithHmac(raw) ?? raw;
                    var saved = JsonSerializer.Deserialize<List<InstalledPrinterConfig>>(json);
                    if (saved != null)
                    {
                        _installedPrinters = saved;
                        foreach (var config in _installedPrinters)
                        {
                            // Skip entries with invalid/missing data
                            if (string.IsNullOrEmpty(config.PipeName) || string.IsNullOrEmpty(config.ServerIp))
                            {
                                AppLogger.Log($"[CLIENT] Skipping invalid config entry: {config.VirtualPrinterName}");
                                continue;
                            }

                            var listener = new PipeListener(config.PipeName, config.ServerIp, config.TargetPrinterName);
                            listener.Start();
                            _activeListeners.Add(listener);
                        }
                    }
                }
                catch (Exception ex) { AppLogger.Error("Failed to load client configuration", ex); }
            }
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
