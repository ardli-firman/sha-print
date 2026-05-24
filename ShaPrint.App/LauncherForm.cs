using System;
using System.Drawing;
using System.IO;
using System.Text.Json;
using System.Windows.Forms;
using ShaPrint.Server;
using ShaPrint.Client;
using MaterialSkin;
using MaterialSkin.Controls;

namespace ShaPrint.App
{
    public class LauncherForm : MaterialForm
    {
        private MaterialButton btnServer;
        private MaterialButton btnClient;
        private string _modeFile;
        private bool _isStartup;
        public bool HasLaunchedMode { get; private set; } = false;

        private string GetConfigPath(string fileName)
        {
            string dir = Path.Combine(Environment.GetFolderPath(Environment.SpecialFolder.LocalApplicationData), "ShaPrint");
            if (!Directory.Exists(dir)) Directory.CreateDirectory(dir);
            return Path.Combine(dir, fileName);
        }

        public LauncherForm(bool isStartup = false)
        {
            _isStartup = isStartup;
            _modeFile = GetConfigPath("AppMode.json");
            this.Text = "ShaPrint - Choose Mode";
            this.Size = new Size(400, 320);
            this.StartPosition = FormStartPosition.CenterScreen;
            this.MaximizeBox = false;
            try { this.Icon = Icon.ExtractAssociatedIcon(Application.ExecutablePath); } catch { }

            var materialSkinManager = MaterialSkinManager.Instance;
            materialSkinManager.AddFormToManage(this);
            materialSkinManager.Theme = MaterialSkinManager.Themes.LIGHT;
            materialSkinManager.ColorScheme = new ColorScheme(
                Primary.Blue600, Primary.Blue700,
                Primary.Blue200, Accent.LightBlue200,
                TextShade.WHITE
            );

            var mainLayout = new TableLayoutPanel
            {
                Dock = DockStyle.Fill,
                RowCount = 3,
                ColumnCount = 1,
                Padding = new Padding(20),
                BackColor = Color.Transparent
            };
            mainLayout.ColumnStyles.Add(new ColumnStyle(SizeType.Percent, 100F));

            MaterialLabel lblInfo = new MaterialLabel
            {
                Text = "Welcome to ShaPrint!\n\nDo you want this PC to act as a Server (Host Printer) or a Client (Send Print Jobs)?",
                FontType = MaterialSkinManager.fontType.Subtitle1,
                AutoSize = true,
                TextAlign = ContentAlignment.MiddleCenter,
                Anchor = AnchorStyles.None,
                Margin = new Padding(0, 0, 0, 20)
            };
            mainLayout.Controls.Add(lblInfo, 0, 0);

            var buttonLayout = new TableLayoutPanel
            {
                Dock = DockStyle.Fill,
                RowCount = 1,
                ColumnCount = 2,
                AutoSize = true
            };
            buttonLayout.ColumnStyles.Add(new ColumnStyle(SizeType.Percent, 50F));
            buttonLayout.ColumnStyles.Add(new ColumnStyle(SizeType.Percent, 50F));

            btnServer = new MaterialButton
            {
                Text = "Run as Server",
                AutoSize = false,
                Size = new Size(130, 45),
                Anchor = AnchorStyles.None,
                Type = MaterialButton.MaterialButtonType.Contained,
                UseAccentColor = false
            };
            btnServer.Click += (s, e) => LaunchMode("Server");
            buttonLayout.Controls.Add(btnServer, 0, 0);

            btnClient = new MaterialButton
            {
                Text = "Run as Client",
                AutoSize = false,
                Size = new Size(130, 45),
                Anchor = AnchorStyles.None,
                Type = MaterialButton.MaterialButtonType.Contained,
                UseAccentColor = true
            };
            btnClient.Click += (s, e) => LaunchMode("Client");
            buttonLayout.Controls.Add(btnClient, 1, 0);

            mainLayout.Controls.Add(buttonLayout, 0, 1);
            this.Controls.Add(mainLayout);
        }

        private void LaunchMode(string mode)
        {
            HasLaunchedMode = true;
            SaveMode(mode);
            this.Hide();

            if (mode == "Server")
            {
                var form = new MainForm(_isStartup);
                form.FormClosed += (s, e) => this.Close();
                form.Show();
            }
            else
            {
                var form = new ClientForm(_isStartup);
                form.FormClosed += (s, e) => this.Close();
                form.Show();
            }
        }

        private void SaveMode(string mode)
        {
            try
            {
                File.WriteAllText(_modeFile, JsonSerializer.Serialize(mode));
            }
            catch { }
        }

        public void CheckSavedModeAndLaunch()
        {
            if (File.Exists(_modeFile))
            {
                try
                {
                    string json = File.ReadAllText(_modeFile);
                    string? mode = JsonSerializer.Deserialize<string>(json);
                    
                    if (mode == "Server" || mode == "Client")
                    {
                        this.Opacity = 0; // Hide it quickly
                        LaunchMode(mode);
                        return;
                    }
                }
                catch { }
            }
            
            // Show self if no valid mode
            this.ShowDialog();
        }
    }
}
