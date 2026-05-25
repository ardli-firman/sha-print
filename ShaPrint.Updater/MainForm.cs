using System;
using System.Diagnostics;
using System.IO;
using System.Net.Http;
using System.Threading.Tasks;
using System.Windows.Forms;

namespace ShaPrint.Updater
{
    public partial class MainForm : Form
    {
        private readonly string _downloadUrl;
        private ProgressBar _progressBar;
        private Label _lblStatus;

        public MainForm(string downloadUrl)
        {
            InitializeComponent();
            _downloadUrl = downloadUrl;
            
            // Set up UI
            this.Text = "ShaPrint Updater";
            this.Size = new System.Drawing.Size(400, 150);
            this.StartPosition = FormStartPosition.CenterScreen;
            this.FormBorderStyle = FormBorderStyle.FixedDialog;
            this.MaximizeBox = false;

            _lblStatus = new Label
            {
                Text = "Downloading update...",
                AutoSize = true,
                Location = new System.Drawing.Point(20, 20)
            };
            this.Controls.Add(_lblStatus);

            _progressBar = new ProgressBar
            {
                Location = new System.Drawing.Point(20, 50),
                Size = new System.Drawing.Size(340, 25),
                Style = ProgressBarStyle.Continuous
            };
            this.Controls.Add(_progressBar);

            this.Load += MainForm_Load;
        }

        private async void MainForm_Load(object? sender, EventArgs e)
        {
            await DownloadAndInstallAsync();
        }

        private async Task DownloadAndInstallAsync()
        {
            string tempFile = Path.Combine(Path.GetTempPath(), "ShaPrint_Update.exe");

            try
            {
                using var client = new HttpClient();
                using var response = await client.GetAsync(_downloadUrl, HttpCompletionOption.ResponseHeadersRead);
                response.EnsureSuccessStatusCode();

                var totalBytes = response.Content.Headers.ContentLength ?? -1L;
                var canReportProgress = totalBytes != -1;

                using (var contentStream = await response.Content.ReadAsStreamAsync())
                using (var fileStream = new FileStream(tempFile, FileMode.Create, FileAccess.Write, FileShare.None, 8192, true))
                {
                    var totalRead = 0L;
                    var buffer = new byte[8192];
                    var isMoreToRead = true;

                    do
                    {
                        var read = await contentStream.ReadAsync(buffer, 0, buffer.Length);
                        if (read == 0)
                        {
                            isMoreToRead = false;
                        }
                        else
                        {
                            await fileStream.WriteAsync(buffer, 0, read);

                            totalRead += read;
                            if (canReportProgress)
                            {
                                int percentage = (int)((totalRead * 100) / totalBytes);
                                _progressBar.Value = percentage;
                                _lblStatus.Text = $"Downloading update... {percentage}%";
                            }
                        }
                    }
                    while (isMoreToRead);
                }

                _lblStatus.Text = "Installing update...";
                _progressBar.Style = ProgressBarStyle.Marquee;

                // Close the main application to avoid file locks
                Process[] processes = Process.GetProcessesByName("ShaPrint.WpfApp");
                foreach (var p in processes)
                {
                    p.Kill();
                    p.WaitForExit(3000);
                }

                // Run installer silently
                ProcessStartInfo psi = new ProcessStartInfo
                {
                    FileName = tempFile,
                    // Use /CLOSEAPPLICATIONS so Inno Setup kills anything still lingering.
                    // Also pass a custom parameter to tell Inno Setup to launch the app after silent install.
                    Arguments = "/VERYSILENT /SUPPRESSMSGBOXES /FORCECLOSEAPPLICATIONS /LAUNCHAFTER",
                    UseShellExecute = true
                };
                Process.Start(psi);

                // Exit updater immediately so it releases its own file lock
                Environment.Exit(0);
            }
            catch (Exception ex)
            {
                MessageBox.Show($"Failed to download or install update: {ex.Message}", "Updater Error", MessageBoxButtons.OK, MessageBoxIcon.Error);
                Environment.Exit(1);
            }
        }
    }
}
