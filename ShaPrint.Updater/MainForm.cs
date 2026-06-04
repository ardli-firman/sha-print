using System;
using System.Diagnostics;
using System.Drawing;
using System.Drawing.Drawing2D;
using System.IO;
using System.Net.Http;
using System.Threading.Tasks;
using System.Windows.Forms;

namespace ShaPrint.Updater
{
    public partial class MainForm : Form
    {
        private readonly string _downloadUrl;
        private ProgressBar _progressBar = null!;
        private Label _lblStatus = null!;
        private Label _lblDetail = null!;
        private Label _lblTitle = null!;
        private Label _lblSubtitle = null!;
        private Panel _headerPanel = null!;
        private Panel _bodyPanel = null!;

        // Colors
        private static readonly Color HeaderColor = Color.FromArgb(30, 39, 73);
        private static readonly Color HeaderAccent = Color.FromArgb(99, 102, 241);
        private static readonly Color BodyBgColor = Color.FromArgb(248, 250, 252);
        private static readonly Color TextPrimary = Color.FromArgb(30, 41, 59);
        private static readonly Color TextSecondary = Color.FromArgb(100, 116, 139);
        private static readonly Color ProgressBg = Color.FromArgb(226, 232, 240);
        private static readonly Color ProgressFill = Color.FromArgb(99, 102, 241);
        private static readonly Color ProgressGlow = Color.FromArgb(139, 92, 246);

        public MainForm(string downloadUrl)
        {
            InitializeComponent();
            _downloadUrl = downloadUrl;

            // Form setup
            this.Text = "ShaPrint Updater";
            this.Size = new Size(500, 300);
            this.StartPosition = FormStartPosition.CenterScreen;
            this.FormBorderStyle = FormBorderStyle.None;
            this.MaximizeBox = false;
            this.BackColor = BodyBgColor;
            this.DoubleBuffered = true;

            // Add rounded corners and shadow via region
            this.Paint += MainForm_Paint;

            BuildUI();
            this.Load += MainForm_Load;
        }

        private void BuildUI()
        {
            // === HEADER PANEL ===
            _headerPanel = new Panel
            {
                Dock = DockStyle.Top,
                Height = 90,
                BackColor = HeaderColor
            };
            _headerPanel.Paint += HeaderPanel_Paint;

            // Title
            _lblTitle = new Label
            {
                Text = "ShaPrint",
                Font = new Font("Segoe UI Semibold", 16f, FontStyle.Bold),
                ForeColor = Color.White,
                Location = new Point(24, 18),
                AutoSize = true,
                BackColor = Color.Transparent
            };
            _headerPanel.Controls.Add(_lblTitle);

            // Subtitle
            _lblSubtitle = new Label
            {
                Text = "Software Updater",
                Font = new Font("Segoe UI", 9f, FontStyle.Regular),
                ForeColor = Color.FromArgb(148, 163, 184),
                Location = new Point(24, 52),
                AutoSize = true,
                BackColor = Color.Transparent
            };
            _headerPanel.Controls.Add(_lblSubtitle);

            // === BODY PANEL ===
            _bodyPanel = new Panel
            {
                Dock = DockStyle.Fill,
                BackColor = BodyBgColor,
                Padding = new Padding(32, 24, 32, 24)
            };

            // Status label
            _lblStatus = new Label
            {
                Text = "Preparing to download update...",
                Font = new Font("Segoe UI Semibold", 11f, FontStyle.Bold),
                ForeColor = TextPrimary,
                Location = new Point(32, 24),
                AutoSize = true
            };
            _bodyPanel.Controls.Add(_lblStatus);

            // Detail label (percentage + speed)
            _lblDetail = new Label
            {
                Text = "",
                Font = new Font("Segoe UI", 9f, FontStyle.Regular),
                ForeColor = TextSecondary,
                Location = new Point(32, 50),
                AutoSize = true
            };
            _bodyPanel.Controls.Add(_lblDetail);

            // Custom progress bar
            _progressBar = new ProgressBar
            {
                Location = new Point(32, 82),
                Size = new Size(420, 8),
                Style = ProgressBarStyle.Continuous,
                Minimum = 0,
                Maximum = 100
            };
            // Hide the default progress bar and paint our own
            _progressBar.Visible = false;
            _bodyPanel.Controls.Add(_progressBar);

            // Custom painted progress bar replacement
            var customProgress = new Panel
            {
                Location = new Point(32, 82),
                Size = new Size(420, 8),
                BackColor = ProgressBg
            };
            customProgress.Paint += CustomProgress_Paint;
            customProgress.Tag = "progress_visual";
            _bodyPanel.Controls.Add(customProgress);

            // Info text at bottom
            var lblInfo = new Label
            {
                Text = "Please do not close this window while updating.",
                Font = new Font("Segoe UI", 8.5f, FontStyle.Regular),
                ForeColor = TextSecondary,
                Location = new Point(32, 110),
                AutoSize = true
            };
            _bodyPanel.Controls.Add(lblInfo);

            // Add panels to form
            this.Controls.Add(_bodyPanel);
            this.Controls.Add(_headerPanel);
        }

        private void MainForm_Paint(object? sender, PaintEventArgs e)
        {
            // Draw rounded border
            var g = e.Graphics;
            g.SmoothingMode = SmoothingMode.AntiAlias;

            using var pen = new Pen(Color.FromArgb(200, 200, 210), 1);
            g.DrawRectangle(pen, 0, 0, Width - 1, Height - 1);
        }

        private void HeaderPanel_Paint(object? sender, PaintEventArgs e)
        {
            var g = e.Graphics;
            g.SmoothingMode = SmoothingMode.AntiAlias;

            // Gradient accent line at bottom of header
            using var gradientBrush = new LinearGradientBrush(
                new Rectangle(0, _headerPanel.Height - 3, _headerPanel.Width, 3),
                HeaderAccent, ProgressGlow, LinearGradientMode.Horizontal);
            g.FillRectangle(gradientBrush, 0, _headerPanel.Height - 3, _headerPanel.Width, 3);

            // Draw a small icon/badge
            using var iconBrush = new SolidBrush(HeaderAccent);
            g.FillEllipse(iconBrush, 445, 30, 28, 28);
            using var iconFont = new Font("Segoe UI", 12f, FontStyle.Bold);
            var iconText = "↓";
            var iconSize = g.MeasureString(iconText, iconFont);
            g.DrawString(iconText, iconFont, Brushes.White,
                445 + (28 - iconSize.Width) / 2, 30 + (28 - iconSize.Height) / 2);
        }

        private int _currentProgress = 0;

        private void CustomProgress_Paint(object? sender, PaintEventArgs e)
        {
            if (sender is not Panel panel) return;
            var g = e.Graphics;
            g.SmoothingMode = SmoothingMode.AntiAlias;

            int fillWidth = (int)(panel.Width * _currentProgress / 100.0);
            if (fillWidth > 0)
            {
                var fillRect = new Rectangle(0, 0, fillWidth, panel.Height);
                using var brush = new LinearGradientBrush(
                    fillRect, ProgressFill, ProgressGlow, LinearGradientMode.Horizontal);
                
                // Rounded rectangle for progress fill
                using var path = CreateRoundedRect(fillRect, 4);
                g.FillPath(brush, path);

                // Glow effect at the leading edge
                if (fillWidth > 4)
                {
                    using var glowBrush = new SolidBrush(Color.FromArgb(80, 255, 255, 255));
                    g.FillEllipse(glowBrush, fillWidth - 8, -2, 12, 12);
                }
            }

            // Background rounded rect (draw border)
            using var borderPath = CreateRoundedRect(new Rectangle(0, 0, panel.Width - 1, panel.Height - 1), 4);
            using var borderPen = new Pen(Color.FromArgb(200, 210, 220), 1);
            g.DrawPath(borderPen, borderPath);
        }

        private GraphicsPath CreateRoundedRect(Rectangle bounds, int radius)
        {
            var path = new GraphicsPath();
            int d = radius * 2;
            path.AddArc(bounds.X, bounds.Y, d, d, 180, 90);
            path.AddArc(bounds.Right - d, bounds.Y, d, d, 270, 90);
            path.AddArc(bounds.Right - d, bounds.Bottom - d, d, d, 0, 90);
            path.AddArc(bounds.X, bounds.Bottom - d, d, d, 90, 90);
            path.CloseFigure();
            return path;
        }

        private void UpdateProgress(int percentage)
        {
            _currentProgress = percentage;
            _progressBar.Value = percentage;
            _lblStatus.Text = $"Downloading update... {percentage}%";
            _lblDetail.Text = $"{percentage}% complete";

            // Trigger repaint of custom progress
            foreach (Control c in _bodyPanel.Controls)
            {
                if (c.Tag?.ToString() == "progress_visual")
                {
                    c.Invalidate();
                    break;
                }
            }
        }

        private void MainForm_Load(object? sender, EventArgs e)
        {
            // Apply rounded corners to the form
            Region = Region.FromHrgn(CreateRoundRectRgn(0, 0, Width, Height, 12, 12));
            DownloadAndInstallAsync().ConfigureAwait(true);
        }

        [System.Runtime.InteropServices.DllImport("gdi32.dll")]
        private static extern IntPtr CreateRoundRectRgn(int nLeftRect, int nTopRect,
            int nRightRect, int nBottomRect, int nWidthEllipse, int nHeightEllipse);

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

                string totalSizeStr = canReportProgress ? FormatBytes(totalBytes) : "";

                using (var contentStream = await response.Content.ReadAsStreamAsync())
                using (var fileStream = new FileStream(tempFile, FileMode.Create, FileAccess.Write, FileShare.None, 8192, true))
                {
                    var totalRead = 0L;
                    var buffer = new byte[8192];
                    var isMoreToRead = true;
                    var stopwatch = Stopwatch.StartNew();

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
                                UpdateProgress(percentage);

                                // Calculate speed
                                var elapsed = stopwatch.Elapsed.TotalSeconds;
                                if (elapsed > 0)
                                {
                                    var speed = totalRead / elapsed;
                                    _lblDetail.Text = $"{percentage}%  •  {FormatBytes(totalRead)} / {totalSizeStr}  •  {FormatBytes((long)speed)}/s";
                                }
                            }
                        }
                    }
                    while (isMoreToRead);
                }

                // Update UI for install phase
                _lblStatus.Text = "Installing update...";
                _lblDetail.Text = "Please wait while the update is being installed.";
                _progressBar.Style = ProgressBarStyle.Marquee;

                // Animate progress bar to marquee-like visual
                foreach (Control c in _bodyPanel.Controls)
                {
                    if (c.Tag?.ToString() == "progress_visual")
                    {
                        _currentProgress = 100;
                        c.Invalidate();
                        break;
                    }
                }

                // Close the main application to avoid file locks
                Process[] processes = Process.GetProcessesByName("ShaPrint");
                foreach (var p in processes)
                {
                    p.Kill();
                    p.WaitForExit(3000);
                }

                // Run installer silently
                ProcessStartInfo psi = new ProcessStartInfo
                {
                    FileName = tempFile,
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

        private static string FormatBytes(long bytes)
        {
            string[] sizes = { "B", "KB", "MB", "GB" };
            int order = 0;
            double size = bytes;
            while (size >= 1024 && order < sizes.Length - 1)
            {
                order++;
                size /= 1024;
            }
            return $"{size:0.##} {sizes[order]}";
        }
    }
}
