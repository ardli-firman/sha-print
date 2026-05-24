using System;
using System.Collections.Generic;
using System.Drawing;
using System.Linq;
using System.Windows.Forms;
using System.IO;
using System.Text.Json;
using ShaPrint.Core;
using ShaPrint.App;
using MaterialSkin;
using MaterialSkin.Controls;
using FontAwesome.Sharp;

namespace ShaPrint.Server
{
    public class MainForm : MaterialForm
    {
        private CheckedListBox clbPrinters;
        private MaterialButton btnToggleServer;
        private MaterialLabel lblStatus;
        private MaterialCheckbox chkRunOnStartup;
        private MaterialCheckbox chkAutoUpdate;
        private MaterialButton btnSettings;
        private NotifyIcon trayIcon;
        private ContextMenuStrip trayMenu;
        private DiscoveryServer _discoveryServer;
        private PrintReceiver _printReceiver;
        private bool _isRunning = false;
        private bool _startHidden;
        private string _configFile;

        private string GetConfigPath(string fileName)
        {
            string dir = Path.Combine(Environment.GetFolderPath(Environment.SpecialFolder.LocalApplicationData), "ShaPrint");
            if (!Directory.Exists(dir)) Directory.CreateDirectory(dir);
            return Path.Combine(dir, fileName);
        }

        public MainForm(bool startHidden = false)
        {
            _startHidden = startHidden;
            _configFile = GetConfigPath("ServerConfig.json");
            _discoveryServer = new DiscoveryServer();
            _printReceiver = new PrintReceiver();

            var materialSkinManager = MaterialSkinManager.Instance;
            materialSkinManager.AddFormToManage(this);
            materialSkinManager.Theme = MaterialSkinManager.Themes.LIGHT;
            materialSkinManager.ColorScheme = new ColorScheme(
                Primary.Green600, Primary.Green700,
                Primary.Green200, Accent.LightGreen200,
                TextShade.WHITE);
            
            InitializeComponent();
            LoadPrinters();
            LoadConfiguration();

            // Auto-check firewall rules on startup
            FirewallManager.CheckAndAddFirewallRules();
        }

        private void InitializeComponent()
        {
            this.Text = "ShaPrint Server";
            this.Size = new Size(450, 550);
            this.MinimumSize = new Size(450, 550);
            this.StartPosition = FormStartPosition.CenterScreen;
            try { this.Icon = System.Drawing.Icon.ExtractAssociatedIcon(Application.ExecutablePath); }
            catch (Exception ex) { ShaPrint.Core.AppLogger.Error("Failed to load application icon", ex); }

            var mainPanel = new TableLayoutPanel
            {
                Dock = DockStyle.Fill,
                Padding = new Padding(20),
                RowCount = 6,
                ColumnCount = 1,
                BackColor = Color.White
            };
            mainPanel.ColumnStyles.Add(new ColumnStyle(SizeType.Percent, 100F));
            mainPanel.RowStyles.Add(new RowStyle(SizeType.AutoSize)); // 0: Label
            mainPanel.RowStyles.Add(new RowStyle(SizeType.Percent, 100F)); // 1: CheckedListBox
            mainPanel.RowStyles.Add(new RowStyle(SizeType.AutoSize)); // 2: Server Controls (Start, Status, Switch Mode)
            mainPanel.RowStyles.Add(new RowStyle(SizeType.AutoSize)); // 3: Checkboxes & Actions
            mainPanel.RowStyles.Add(new RowStyle(SizeType.Absolute, 120F)); // 4: Log
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

            MaterialLabel lblInfo = new MaterialLabel
            {
                Text = "Select printers to expose to the network:",
                AutoSize = true,
                Margin = new Padding(0, 0, 0, 10)
            };
            mainPanel.Controls.Add(lblInfo, 0, 0);

            clbPrinters = new CheckedListBox
            {
                Dock = DockStyle.Fill,
                CheckOnClick = true,
                BorderStyle = BorderStyle.FixedSingle,
                Margin = new Padding(0, 0, 0, 15),
                Font = new Font("Segoe UI", 9.5F)
            };
            mainPanel.Controls.Add(clbPrinters, 0, 1);

            var controlPanel = new TableLayoutPanel
            {
                Dock = DockStyle.Fill,
                AutoSize = true,
                RowCount = 1,
                ColumnCount = 3,
                Margin = new Padding(0, 0, 0, 15)
            };
            controlPanel.ColumnStyles.Add(new ColumnStyle(SizeType.Percent, 33.33F));
            controlPanel.ColumnStyles.Add(new ColumnStyle(SizeType.Percent, 33.33F));
            controlPanel.ColumnStyles.Add(new ColumnStyle(SizeType.Percent, 33.34F));
            mainPanel.Controls.Add(controlPanel, 0, 2);

            btnToggleServer = new MaterialButton
            {
                Text = "Start Server",
                AutoSize = false,
                Dock = DockStyle.Fill,
                Type = MaterialButton.MaterialButtonType.Contained,
                UseAccentColor = false,
                Margin = new Padding(0, 0, 5, 0)
            };
            btnToggleServer.Click += BtnToggleServer_Click;
            controlPanel.Controls.Add(btnToggleServer, 0, 0);

            MaterialButton btnSwitchMode = new MaterialButton
            {
                Text = "Switch Mode",
                AutoSize = false,
                Dock = DockStyle.Fill,
                Type = MaterialButton.MaterialButtonType.Text,
                UseAccentColor = false,
                Margin = new Padding(5, 0, 5, 0)
            };
            btnSwitchMode.Click += (s, e) => SwitchMode();
            controlPanel.Controls.Add(btnSwitchMode, 1, 0);

            lblStatus = new MaterialLabel
            {
                Text = "Status: Stopped",
                AutoSize = true,
                Margin = new Padding(5, 8, 0, 0)
            };
            controlPanel.Controls.Add(lblStatus, 2, 0);

            var settingsLayout = new TableLayoutPanel { Dock = DockStyle.Fill, Padding = new Padding(20), RowCount = 4, ColumnCount = 1 };
            settingsLayout.ColumnStyles.Add(new ColumnStyle(SizeType.Percent, 100F));
            
            var lblSetTitle = new MaterialLabel { Text = "Configuration", FontType = MaterialSkinManager.fontType.H5, AutoSize = true, Margin = new Padding(0,0,0,20) };
            settingsLayout.Controls.Add(lblSetTitle, 0, 0);

            chkRunOnStartup = new MaterialCheckbox { Text = "Run Server Automatically on Windows Startup", AutoSize = true, Checked = ShaPrint.App.StartupManager.IsStartupEnabled() };
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

            var txtLog = new TextBox
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
            mainPanel.Controls.Add(txtLog, 0, 4);

            ShaPrint.Core.AppLogger.OnLog += (msg) => 
            {
                if (this.IsHandleCreated)
                {
                    this.Invoke(new Action(() => {
                        txtLog.AppendText(msg + Environment.NewLine);
                    }));
                }
            };

            trayMenu = new ContextMenuStrip();
            trayMenu.Items.Add("Open", null, (s, e) => { this.Show(); this.WindowState = FormWindowState.Normal; });
            trayMenu.Items.Add("Exit", null, (s, e) => { Application.Exit(); });

            trayIcon = new NotifyIcon();
            trayIcon.Text = "ShaPrint Server";
            try { trayIcon.Icon = System.Drawing.Icon.ExtractAssociatedIcon(Application.ExecutablePath); } catch { trayIcon.Icon = SystemIcons.Application; }
            trayIcon.ContextMenuStrip = trayMenu;
            trayIcon.Visible = true;
            trayIcon.DoubleClick += (s, e) => { this.Show(); this.WindowState = FormWindowState.Normal; };
        }

        private void LoadPrinters()
        {
            var printers = SpoolerApi.GetLocalPrinters();
            clbPrinters.Items.Clear();
            foreach (var p in printers)
            {
                clbPrinters.Items.Add(p);
            }
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

        private void MainForm_Load(object? sender, EventArgs e)
        {
            if (_isRunning)
            {
                StopServer();
            }
        }

        private void BtnToggleServer_Click(object? sender, EventArgs e)
        {
            if (_isRunning)
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
            var selectedPrinters = new List<string>();
            foreach (var item in clbPrinters.CheckedItems)
            {
                selectedPrinters.Add(item.ToString()!);
            }

            if (selectedPrinters.Count == 0)
            {
                MessageBox.Show("Please select at least one printer to expose.", "Warning", MessageBoxButtons.OK, MessageBoxIcon.Warning);
                return;
            }

            _discoveryServer.SetExposedPrinters(selectedPrinters);
            _discoveryServer.Start();
            _printReceiver.Start();

            _isRunning = true;
            btnToggleServer.Text = "Stop Server";
            lblStatus.Text = "Status: Running";
            lblStatus.ForeColor = Color.Green;
            clbPrinters.Enabled = false;

            SaveConfiguration(selectedPrinters);
        }

        private void StopServer()
        {
            _discoveryServer.Stop();
            _printReceiver.Stop();
            FirewallManager.RemoveFirewallRules();

            _isRunning = false;
            btnToggleServer.Text = "Start Server";
            lblStatus.Text = "Status: Stopped";
            lblStatus.ForeColor = Color.Black;
            clbPrinters.Enabled = true;
        }

        private bool _isSwitchingMode = false;
        
        private void SwitchMode()
        {
            var result = MessageBox.Show("Are you sure you want to switch to Client Mode?\n\nThis will restart the application.", "Switch Mode", MessageBoxButtons.YesNo, MessageBoxIcon.Question);
            if (result == DialogResult.Yes)
            {
                _isSwitchingMode = true;
                if (_isRunning) StopServer();
                trayIcon.Visible = false;
                
                try
                {
                    if (File.Exists(GetConfigPath("AppMode.json")))
                        File.Delete(GetConfigPath("AppMode.json"));
                }
                catch (Exception ex) { ShaPrint.Core.AppLogger.Error("Failed to delete app mode configuration", ex); }

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
                trayIcon.ShowBalloonTip(2000, "ShaPrint Server", "Server is running in the background.", ToolTipIcon.Info);
            }
            else
            {
                if (_isRunning) StopServer();
                trayIcon.Visible = false;
            }
        }

        private void LoadConfiguration()
        {
            if (!File.Exists(_configFile))
                return;

            try
            {
                string raw = File.ReadAllText(_configFile);

                // HMAC-wrapped config (v2+): reject tampered, fall back only for legacy
                ConfigUnwrapResult result = ShaPrint.Core.CryptoHelper.UnwrapConfigWithHmac(raw, out string? json);
                if (result == ConfigUnwrapResult.Valid)
                {
                    raw = json!;
                }
                else if (result == ConfigUnwrapResult.Tampered)
                {
                    ShaPrint.Core.AppLogger.Error("[SERVER] Config file HMAC verification FAILED — possible tampering. Rejecting config.");
                    return;
                }
                // LegacyNoHmac: use raw plaintext (unwrapped)

                var savedPrinters = JsonSerializer.Deserialize<List<string>>(raw);
                if (savedPrinters != null && savedPrinters.Count > 0)
                {
                    for (int i = 0; i < clbPrinters.Items.Count; i++)
                    {
                        if (savedPrinters.Contains(clbPrinters.Items[i].ToString()!))
                        {
                            clbPrinters.SetItemChecked(i, true);
                        }
                    }
                    StartServer();
                }
            }
            catch (Exception ex) { ShaPrint.Core.AppLogger.Error("Failed to load server configuration", ex); }
        }

        private void SaveConfiguration(List<string> printers)
        {
            try
            {
                string json = JsonSerializer.Serialize(printers);
                string wrapped = ShaPrint.Core.CryptoHelper.WrapConfigWithHmac(json);
                File.WriteAllText(_configFile, wrapped);
            }
            catch (Exception ex) { ShaPrint.Core.AppLogger.Error("Failed to save server configuration", ex); }
        }

        private ImageList CreateIcons()
        {
            var il = new ImageList { ImageSize = new Size(24, 24), ColorDepth = ColorDepth.Depth32Bit };
            
            il.Images.Add("home", IconChar.Home.ToBitmap(Color.DimGray, 24));
            il.Images.Add("settings", IconChar.Cogs.ToBitmap(Color.DimGray, 24));
            il.Images.Add("about", IconChar.InfoCircle.ToBitmap(Color.DimGray, 24));

            return il;
        }
    }
}
