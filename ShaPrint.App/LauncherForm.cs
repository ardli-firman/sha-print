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
            this.Size = new Size(450, 480);
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
                RowCount = 6,
                ColumnCount = 1,
                Padding = new Padding(20, 80, 20, 20),
                BackColor = Color.White
            };
            mainLayout.ColumnStyles.Add(new ColumnStyle(SizeType.Percent, 100F));
            
            mainLayout.RowStyles.Add(new RowStyle(SizeType.AutoSize)); // Title 1
            mainLayout.RowStyles.Add(new RowStyle(SizeType.AutoSize)); // Title 2
            mainLayout.RowStyles.Add(new RowStyle(SizeType.AutoSize)); // Server Btn
            mainLayout.RowStyles.Add(new RowStyle(SizeType.Percent, 50F)); // Server Desc
            mainLayout.RowStyles.Add(new RowStyle(SizeType.AutoSize)); // Client Btn
            mainLayout.RowStyles.Add(new RowStyle(SizeType.Percent, 50F)); // Client Desc

            MaterialLabel lblInfo1 = new MaterialLabel
            {
                Text = "Welcome to ShaPrint",
                FontType = MaterialSkinManager.fontType.H5,
                AutoSize = true,
                Anchor = AnchorStyles.None,
                Margin = new Padding(0, 0, 0, 5)
            };
            mainLayout.Controls.Add(lblInfo1, 0, 0);

            MaterialLabel lblInfo2 = new MaterialLabel
            {
                Text = "Select an operation mode:",
                FontType = MaterialSkinManager.fontType.Subtitle1,
                AutoSize = true,
                Anchor = AnchorStyles.None,
                Margin = new Padding(0, 0, 0, 30)
            };
            mainLayout.Controls.Add(lblInfo2, 0, 1);

            btnServer = new MaterialButton
            {
                Text = "RUN AS SERVER",
                AutoSize = false,
                Size = new Size(220, 50),
                Anchor = AnchorStyles.None,
                Type = MaterialButton.MaterialButtonType.Contained,
                UseAccentColor = false,
                Margin = new Padding(0, 0, 0, 10)
            };
            btnServer.Click += (s, e) => LaunchMode("Server");
            mainLayout.Controls.Add(btnServer, 0, 2);
            
            var lblServerDesc = new MaterialLabel
            {
                Text = "Host your local printers to the network.",
                FontType = MaterialSkinManager.fontType.Body2,
                AutoSize = true,
                Anchor = AnchorStyles.Top,
                Margin = new Padding(0, 0, 0, 20)
            };
            mainLayout.Controls.Add(lblServerDesc, 0, 3);

            btnClient = new MaterialButton
            {
                Text = "RUN AS CLIENT",
                AutoSize = false,
                Size = new Size(220, 50),
                Anchor = AnchorStyles.None,
                Type = MaterialButton.MaterialButtonType.Contained,
                UseAccentColor = true,
                Margin = new Padding(0, 0, 0, 10)
            };
            btnClient.Click += (s, e) => LaunchMode("Client");
            mainLayout.Controls.Add(btnClient, 0, 4);
            
            var lblClientDesc = new MaterialLabel
            {
                Text = "Connect and print to a ShaPrint Server.",
                FontType = MaterialSkinManager.fontType.Body2,
                AutoSize = true,
                Anchor = AnchorStyles.Top,
                Margin = new Padding(0, 0, 0, 10)
            };
            mainLayout.Controls.Add(lblClientDesc, 0, 5);

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
