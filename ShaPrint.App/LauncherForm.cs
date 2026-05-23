using System;
using System.Drawing;
using System.IO;
using System.Text.Json;
using System.Windows.Forms;
using ShaPrint.Server;
using ShaPrint.Client;

namespace ShaPrint.App
{
    public class LauncherForm : Form
    {
        private Button btnServer;
        private Button btnClient;
        private readonly string _modeFile = "AppMode.json";

        public LauncherForm()
        {
            this.Text = "ShaPrint - Choose Mode";
            this.Size = new Size(350, 250);
            this.StartPosition = FormStartPosition.CenterScreen;
            this.FormBorderStyle = FormBorderStyle.FixedDialog;
            this.MaximizeBox = false;

            Label lblInfo = new Label();
            lblInfo.Text = "Welcome to ShaPrint!\n\nDo you want this PC to act as a Server (Host Printer) or a Client (Send Print Jobs)?";
            lblInfo.Location = new Point(20, 20);
            lblInfo.Size = new Size(300, 60);
            lblInfo.TextAlign = ContentAlignment.MiddleCenter;
            this.Controls.Add(lblInfo);

            btnServer = new Button();
            btnServer.Text = "Run as Server";
            btnServer.Location = new Point(40, 100);
            btnServer.Size = new Size(120, 50);
            btnServer.Click += (s, e) => LaunchMode("Server");
            this.Controls.Add(btnServer);

            btnClient = new Button();
            btnClient.Text = "Run as Client";
            btnClient.Location = new Point(170, 100);
            btnClient.Size = new Size(120, 50);
            btnClient.Click += (s, e) => LaunchMode("Client");
            this.Controls.Add(btnClient);
        }

        private void LaunchMode(string mode)
        {
            SaveMode(mode);
            this.Hide();

            if (mode == "Server")
            {
                var form = new MainForm();
                form.FormClosed += (s, e) => this.Close();
                form.Show();
            }
            else
            {
                var form = new ClientForm();
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
