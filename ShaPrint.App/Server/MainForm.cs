using System;
using System.Collections.Generic;
using System.Drawing;
using System.Linq;
using System.Windows.Forms;
using System.IO;
using System.Text.Json;

namespace ShaPrint.Server
{
    public class MainForm : Form
    {
        private CheckedListBox clbPrinters;
        private Button btnToggleServer;
        private Label lblStatus;
        private CheckBox chkRunOnStartup;
        private NotifyIcon trayIcon;
        private ContextMenuStrip trayMenu;
        private DiscoveryServer _discoveryServer;
        private PrintReceiver _printReceiver;
        private bool _isRunning = false;
        private readonly string _configFile = "ServerConfig.json";

        public MainForm()
        {
            _discoveryServer = new DiscoveryServer();
            _printReceiver = new PrintReceiver();
            
            InitializeComponent();
            LoadPrinters();
            LoadConfiguration();

            // Auto-check firewall rules on startup
            FirewallManager.CheckAndAddFirewallRules();
        }

        private void InitializeComponent()
        {
            this.Text = "ShaPrint Server";
            this.Size = new Size(400, 300);
            this.StartPosition = FormStartPosition.CenterScreen;

            Label lblInfo = new Label();
            lblInfo.Text = "Select printers to expose to the network:";
            lblInfo.Location = new Point(10, 10);
            lblInfo.AutoSize = true;
            this.Controls.Add(lblInfo);

            clbPrinters = new CheckedListBox();
            clbPrinters.Location = new Point(10, 30);
            clbPrinters.Size = new Size(360, 150);
            clbPrinters.CheckOnClick = true;
            this.Controls.Add(clbPrinters);

            btnToggleServer = new Button();
            btnToggleServer.Text = "Start Server";
            btnToggleServer.Location = new Point(10, 190);
            btnToggleServer.Size = new Size(100, 30);
            btnToggleServer.Click += BtnToggleServer_Click;
            this.Controls.Add(btnToggleServer);

            lblStatus = new Label();
            lblStatus.Text = "Status: Stopped";
            lblStatus.Location = new Point(120, 198);
            lblStatus.AutoSize = true;
            this.Controls.Add(lblStatus);

            Button btnSwitchMode = new Button();
            btnSwitchMode.Text = "Switch to Client Mode";
            btnSwitchMode.Location = new Point(220, 190);
            btnSwitchMode.Size = new Size(150, 30);
            btnSwitchMode.Click += (s, e) => SwitchMode();
            this.Controls.Add(btnSwitchMode);

            chkRunOnStartup = new CheckBox();
            chkRunOnStartup.Text = "Run Server Automatically on Windows Startup";
            chkRunOnStartup.Location = new Point(10, 225);
            chkRunOnStartup.AutoSize = true;
            chkRunOnStartup.Checked = ShaPrint.App.StartupManager.IsStartupEnabled();
            chkRunOnStartup.CheckedChanged += (s, e) => ShaPrint.App.StartupManager.SetStartup(chkRunOnStartup.Checked);
            this.Controls.Add(chkRunOnStartup);

            var txtLog = new TextBox();
            txtLog.Multiline = true;
            txtLog.ReadOnly = true;
            txtLog.ScrollBars = ScrollBars.Vertical;
            txtLog.Location = new Point(10, 255);
            txtLog.Size = new Size(360, 195);
            txtLog.BackColor = Color.Black;
            txtLog.ForeColor = Color.Lime;
            txtLog.Font = new Font("Consolas", 9F);
            this.Controls.Add(txtLog);

            this.Size = new Size(400, 500); // Expanded size for logs

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
            trayIcon.Icon = SystemIcons.Application;
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
                    File.Delete("AppMode.json");
                }
                catch { }

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
            if (File.Exists(_configFile))
            {
                try
                {
                    string json = File.ReadAllText(_configFile);
                    var savedPrinters = JsonSerializer.Deserialize<List<string>>(json);
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
                catch { }
            }
        }

        private void SaveConfiguration(List<string> printers)
        {
            try
            {
                string json = JsonSerializer.Serialize(printers);
                File.WriteAllText(_configFile, json);
            }
            catch { }
        }
    }
}
