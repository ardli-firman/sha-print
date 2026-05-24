using System;
using System.Collections.Generic;
using System.Drawing;
using System.IO;
using System.Linq;
using System.Text.Json;
using System.Windows.Forms;
using ShaPrint.Core;
using ShaPrint.Core.Network;
using ShaPrint.App;
using MaterialSkin;
using MaterialSkin.Controls;
using FontAwesome.Sharp;

namespace ShaPrint.Client
{
    public class ClientForm : MaterialForm
    {
        private MaterialButton btnScan;
        private ListBox lbServers;
        private MaterialButton btnInstall;
        private MaterialButton btnDelete;
        private MaterialLabel lblStatus;
        private MaterialLabel lblIp;
        private MaterialTextBox2 txtServerIp;
        private MaterialCheckbox chkRunOnStartup;
        private MaterialCheckbox chkAutoUpdate;
        private MaterialButton btnSettings;
        private TextBox txtLog;

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

            var materialSkinManager = MaterialSkinManager.Instance;
            materialSkinManager.AddFormToManage(this);
            materialSkinManager.Theme = MaterialSkinManager.Themes.LIGHT;
            materialSkinManager.ColorScheme = new ColorScheme(
                Primary.Blue600, Primary.Blue700,
                Primary.Blue200, Accent.LightBlue200,
                TextShade.WHITE);

            InitializeComponent();
            LoadConfiguration();
        }

        private void InitializeComponent()
        {
            this.Text = "ShaPrint Client";
            this.Size = new Size(550, 650);
            this.MinimumSize = new Size(550, 650);
            this.StartPosition = FormStartPosition.CenterScreen;
            try { this.Icon = System.Drawing.Icon.ExtractAssociatedIcon(Application.ExecutablePath); } catch { }

            var mainPanel = new TableLayoutPanel
            {
                Dock = DockStyle.Fill,
                Padding = new Padding(20),
                RowCount = 7,
                ColumnCount = 1,
                BackColor = Color.White
            };
            mainPanel.ColumnStyles.Add(new ColumnStyle(SizeType.Percent, 100F));
            mainPanel.RowStyles.Add(new RowStyle(SizeType.AutoSize)); // 0: Network Discovery
            mainPanel.RowStyles.Add(new RowStyle(SizeType.Percent, 100F)); // 1: Server ListBox
            mainPanel.RowStyles.Add(new RowStyle(SizeType.AutoSize)); // 2: Install/Delete Buttons
            mainPanel.RowStyles.Add(new RowStyle(SizeType.AutoSize)); // 3: Status
            mainPanel.RowStyles.Add(new RowStyle(SizeType.AutoSize)); // 4: Checkboxes & Actions
            mainPanel.RowStyles.Add(new RowStyle(SizeType.Absolute, 120F)); // 5: Logs
            
            var tabControl = new MaterialTabControl
            {
                Dock = DockStyle.Fill,
                ImageList = CreateIcons()
            };
            this.Controls.Add(tabControl);
            this.DrawerTabControl = tabControl;
            this.DrawerShowIconsWhenHidden = true;
            this.DrawerUseColors = true;
            this.DrawerHighlightWithAccent = true;

            var tabHome = new TabPage("Home") { ImageKey = "home", BackColor = Color.White };
            var tabSettings = new TabPage("Settings") { ImageKey = "settings", BackColor = Color.White };
            var tabAbout = new TabPage("About") { ImageKey = "about", BackColor = Color.White };

            tabControl.TabPages.Add(tabHome);
            tabControl.TabPages.Add(tabSettings);
            tabControl.TabPages.Add(tabAbout);
            
            tabHome.Controls.Add(mainPanel);

            var grpNetwork = new GroupBox
            {
                Text = "Network Discovery",
                Dock = DockStyle.Fill,
                AutoSize = true,
                Margin = new Padding(0, 0, 0, 15),
                Padding = new Padding(10),
                Font = new Font("Segoe UI Semibold", 9F, FontStyle.Bold)
            };
            mainPanel.Controls.Add(grpNetwork, 0, 0);

            var grpLayout = new TableLayoutPanel
            {
                Dock = DockStyle.Fill,
                AutoSize = true,
                RowCount = 2,
                ColumnCount = 3
            };
            grpLayout.ColumnStyles.Add(new ColumnStyle(SizeType.AutoSize));
            grpLayout.ColumnStyles.Add(new ColumnStyle(SizeType.Percent, 100F));
            grpLayout.ColumnStyles.Add(new ColumnStyle(SizeType.AutoSize));
            grpNetwork.Controls.Add(grpLayout);

            var lblHint = new MaterialLabel
            {
                Text = "Hint: If Server is on a different Wi-Fi/VLAN,\nauto-scan won't work. Enter Server IP explicitly.",
                FontType = MaterialSkinManager.fontType.Body2,
                AutoSize = true,
                Margin = new Padding(0, 0, 0, 10)
            };
            grpLayout.Controls.Add(lblHint, 0, 0);
            grpLayout.SetColumnSpan(lblHint, 3);

            lblIp = new MaterialLabel
            {
                Text = "Specific Server IP:",
                AutoSize = true,
                Anchor = AnchorStyles.Left | AnchorStyles.Top,
                Margin = new Padding(0, 12, 5, 0)
            };
            grpLayout.Controls.Add(lblIp, 0, 1);

            txtServerIp = new MaterialTextBox2
            {
                Dock = DockStyle.Fill,
                Margin = new Padding(0, 0, 10, 0),
                UseSystemPasswordChar = false
            };
            grpLayout.Controls.Add(txtServerIp, 1, 1);

            var toolTip = new ToolTip();
            toolTip.SetToolTip(txtServerIp, "Example: 192.168.1.50\nLeave blank for auto-discovery on the same local network.");

            btnScan = new MaterialButton
            {
                Text = "Scan LAN",
                AutoSize = false,
                Size = new Size(140, 36),
                Type = MaterialButton.MaterialButtonType.Contained,
                UseAccentColor = false
            };
            btnScan.Click += BtnScan_Click;
            grpLayout.Controls.Add(btnScan, 2, 1);

            lbServers = new ListBox
            {
                Dock = DockStyle.Fill,
                Margin = new Padding(0, 0, 0, 15),
                Font = new Font("Segoe UI", 9.5F),
                BorderStyle = BorderStyle.FixedSingle
            };
            mainPanel.Controls.Add(lbServers, 0, 1);

            var actionPanel = new TableLayoutPanel
            {
                Dock = DockStyle.Fill,
                AutoSize = true,
                RowCount = 1,
                ColumnCount = 3,
                Margin = new Padding(0, 0, 0, 10)
            };
            actionPanel.ColumnStyles.Add(new ColumnStyle(SizeType.Percent, 33.33F));
            actionPanel.ColumnStyles.Add(new ColumnStyle(SizeType.Percent, 33.33F));
            actionPanel.ColumnStyles.Add(new ColumnStyle(SizeType.Percent, 33.34F));
            mainPanel.Controls.Add(actionPanel, 0, 2);

            btnInstall = new MaterialButton
            {
                Text = "Install Selected",
                AutoSize = false,
                Dock = DockStyle.Fill,
                Type = MaterialButton.MaterialButtonType.Contained,
                UseAccentColor = true,
                Enabled = false,
                Margin = new Padding(0, 0, 5, 0)
            };
            btnInstall.Click += BtnInstall_Click;
            actionPanel.Controls.Add(btnInstall, 0, 0);

            btnDelete = new MaterialButton
            {
                Text = "Delete Selected",
                AutoSize = false,
                Dock = DockStyle.Fill,
                Type = MaterialButton.MaterialButtonType.Outlined,
                UseAccentColor = false,
                Enabled = false,
                Margin = new Padding(5, 0, 5, 0)
            };
            btnDelete.Click += BtnDelete_Click;
            actionPanel.Controls.Add(btnDelete, 1, 0);

            MaterialButton btnSwitchMode = new MaterialButton
            {
                Text = "Switch Mode",
                AutoSize = false,
                Dock = DockStyle.Fill,
                Type = MaterialButton.MaterialButtonType.Text,
                UseAccentColor = false,
                Margin = new Padding(5, 0, 0, 0)
            };
            btnSwitchMode.Click += (s, e) => SwitchMode();
            actionPanel.Controls.Add(btnSwitchMode, 2, 0);

            lblStatus = new MaterialLabel
            {
                Text = "Ready",
                AutoSize = true,
                Margin = new Padding(0, 0, 0, 10)
            };
            mainPanel.Controls.Add(lblStatus, 0, 3);

            var settingsLayout = new TableLayoutPanel { Dock = DockStyle.Fill, Padding = new Padding(20), RowCount = 4, ColumnCount = 1 };
            settingsLayout.ColumnStyles.Add(new ColumnStyle(SizeType.Percent, 100F));
            
            var lblSetTitle = new MaterialLabel { Text = "Configuration", FontType = MaterialSkinManager.fontType.H5, AutoSize = true, Margin = new Padding(0,0,0,20) };
            settingsLayout.Controls.Add(lblSetTitle, 0, 0);

            chkRunOnStartup = new MaterialCheckbox { Text = "Run Automatically on Windows Startup", AutoSize = true, Checked = ShaPrint.App.StartupManager.IsStartupEnabled() };
            chkRunOnStartup.CheckedChanged += (s, e) => ShaPrint.App.StartupManager.SetStartup(chkRunOnStartup.Checked);
            settingsLayout.Controls.Add(chkRunOnStartup, 0, 1);

            chkAutoUpdate = new MaterialCheckbox { Text = "Enable Auto-Update on Startup", AutoSize = true, Checked = AppSettings.Current.AutoUpdateEnabled };
            chkAutoUpdate.CheckedChanged += (s, e) => { AppSettings.Current.AutoUpdateEnabled = chkAutoUpdate.Checked; AppSettings.Save(); };
            settingsLayout.Controls.Add(chkAutoUpdate, 0, 2);

            var btnCheckUpdate = new MaterialButton { Text = "Check for Updates", AutoSize = false, Size = new Size(200, 36), Type = MaterialButton.MaterialButtonType.Contained, Margin = new Padding(0, 20, 0, 0) };
            btnCheckUpdate.Click += async (s, e) => { await UpdateChecker.CheckForUpdatesManualAsync(); };
            settingsLayout.Controls.Add(btnCheckUpdate, 0, 3);
            
            tabSettings.Controls.Add(settingsLayout);

            var aboutLayout = new TableLayoutPanel { Dock = DockStyle.Fill, RowCount = 5, ColumnCount = 1, Padding = new Padding(20), BackColor = Color.White };
            aboutLayout.Controls.Add(new MaterialLabel { Text = "ShaPrint", FontType = MaterialSkinManager.fontType.H4, AutoSize = true }, 0, 0);
            aboutLayout.Controls.Add(new MaterialLabel { Text = $"Version {System.Reflection.Assembly.GetExecutingAssembly().GetName().Version}", FontType = MaterialSkinManager.fontType.Subtitle1, AutoSize = true, Margin = new Padding(0, 5, 0, 10) }, 0, 1);
            aboutLayout.Controls.Add(new MaterialLabel { Text = "Author: ardli-firman", FontType = MaterialSkinManager.fontType.Body1, AutoSize = true }, 0, 2);
            aboutLayout.Controls.Add(new MaterialLabel { Text = "A Virtual Printer and Print Server\nsolution for Windows networks.", FontType = MaterialSkinManager.fontType.Body2, AutoSize = true, Margin = new Padding(0, 10, 0, 0) }, 0, 3);
            tabAbout.Controls.Add(aboutLayout);

            txtLog = new TextBox
            {
                Multiline = true,
                ReadOnly = true,
                ScrollBars = ScrollBars.Vertical,
                Dock = DockStyle.Fill,
                BackColor = Color.FromArgb(240, 240, 240),
                ForeColor = Color.DarkSlateGray,
                Font = new Font("Consolas", 9.5F),
                BorderStyle = BorderStyle.FixedSingle,
                Margin = new Padding(0)
            };
            mainPanel.Controls.Add(txtLog, 0, 5);

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
            try { trayIcon.Icon = System.Drawing.Icon.ExtractAssociatedIcon(Application.ExecutablePath); } catch { trayIcon.Icon = SystemIcons.Application; }
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
                        IsInstalled = isInstalled,
                        IsVerified = !string.IsNullOrEmpty(server.HmacSignature)
                    });
                }
            }

            lblStatus.Text = $"Found {_discoveredServers.Count} server(s).";
            btnScan.Enabled = true;
        }

        private void TxtServerIp_KeyDown(object? sender, KeyEventArgs e)
        {
            if (e.KeyCode == Keys.Enter)
            {
                e.SuppressKeyPress = true;
                BtnScan_Click(sender, e);
            }
        }

        private ImageList CreateIcons()
        {
            var il = new ImageList { ImageSize = new Size(24, 24), ColorDepth = ColorDepth.Depth32Bit };
            
            il.Images.Add("home", IconChar.Home.ToBitmap(Color.DimGray, 24));
            il.Images.Add("settings", IconChar.Cogs.ToBitmap(Color.DimGray, 24));
            il.Images.Add("about", IconChar.InfoCircle.ToBitmap(Color.DimGray, 24));

            return il;
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

                    // HMAC-wrapped config (v2+): reject tampered, fall back only for legacy
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
                    // LegacyNoHmac: use raw plaintext (unwrapped)
                    var saved = JsonSerializer.Deserialize<List<InstalledPrinterConfig>>(raw);
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
            public bool IsVerified { get; set; } = false;
            public override string ToString() =>
                $"{(IsVerified ? "" : "[UNVERIFIED] ")}[{Server.ServerName}] {Printer.Name} {(IsInstalled ? "(INSTALLED)" : "")}";
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
