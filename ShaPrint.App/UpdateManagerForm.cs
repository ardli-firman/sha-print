using System;
using System.Collections.Generic;
using System.Drawing;
using System.Linq;
using System.Threading.Tasks;
using System.Windows.Forms;
using MaterialSkin;
using MaterialSkin.Controls;

namespace ShaPrint.App
{
    public class UpdateManagerForm : MaterialForm
    {
        private MaterialListView lstReleases;
        private MaterialComboBox cmbChannel;
        private MaterialButton btnInstall;
        private MaterialButton btnRefresh;
        private MaterialButton btnClose;
        private MaterialLabel lblStatus;

        private List<GitHubRelease> _releases = new List<GitHubRelease>();

        public UpdateManagerForm()
        {
            var materialSkinManager = MaterialSkinManager.Instance;
            materialSkinManager.AddFormToManage(this);
            InitializeComponent();
        }

        private void InitializeComponent()
        {
            this.Text = "Update Manager";
            this.Size = new Size(600, 500);
            this.StartPosition = FormStartPosition.CenterParent;
            this.MaximizeBox = false;

            var mainPanel = new TableLayoutPanel
            {
                Dock = DockStyle.Fill,
                Padding = new Padding(15, 80, 15, 15),
                RowCount = 4,
                ColumnCount = 1,
                BackColor = Color.White
            };
            mainPanel.RowStyles.Add(new RowStyle(SizeType.AutoSize)); // Top controls
            mainPanel.RowStyles.Add(new RowStyle(SizeType.Percent, 100F)); // List
            mainPanel.RowStyles.Add(new RowStyle(SizeType.AutoSize)); // Status
            mainPanel.RowStyles.Add(new RowStyle(SizeType.AutoSize)); // Bottom buttons
            this.Controls.Add(mainPanel);

            // Top controls
            var topPanel = new FlowLayoutPanel
            {
                Dock = DockStyle.Fill,
                AutoSize = true,
                FlowDirection = FlowDirection.LeftToRight
            };

            var lblChannel = new MaterialLabel
            {
                Text = "Auto-Update Channel:",
                AutoSize = true,
                Margin = new Padding(0, 15, 10, 0)
            };
            topPanel.Controls.Add(lblChannel);

            cmbChannel = new MaterialComboBox
            {
                Width = 200,
                DropDownStyle = ComboBoxStyle.DropDownList
            };
            cmbChannel.Items.Add("Production");
            cmbChannel.Items.Add("Beta");
            cmbChannel.SelectedIndex = AppSettings.Current.Channel == UpdateChannel.Beta ? 1 : 0;
            cmbChannel.SelectedIndexChanged += CmbChannel_SelectedIndexChanged;
            topPanel.Controls.Add(cmbChannel);

            btnRefresh = new MaterialButton
            {
                Text = "Refresh List",
                Type = MaterialButton.MaterialButtonType.Outlined,
                AutoSize = true,
                Margin = new Padding(20, 10, 0, 0)
            };
            btnRefresh.Click += async (s, e) => await LoadReleasesAsync();
            topPanel.Controls.Add(btnRefresh);

            mainPanel.Controls.Add(topPanel, 0, 0);

            // List
            lstReleases = new MaterialListView
            {
                Dock = DockStyle.Fill,
                FullRowSelect = true,
                MultiSelect = false,
                View = View.Details
            };
            lstReleases.Columns.Add("Version", 120);
            lstReleases.Columns.Add("Channel", 100);
            lstReleases.Columns.Add("Date", 150);
            lstReleases.Columns.Add("Name", 180);
            lstReleases.SelectedIndexChanged += LstReleases_SelectedIndexChanged;
            mainPanel.Controls.Add(lstReleases, 0, 1);

            // Status
            lblStatus = new MaterialLabel
            {
                Text = "Loading releases...",
                AutoSize = true,
                Dock = DockStyle.Fill
            };
            mainPanel.Controls.Add(lblStatus, 0, 2);

            // Bottom Buttons
            var btnPanel = new TableLayoutPanel
            {
                Dock = DockStyle.Fill,
                AutoSize = true,
                RowCount = 1,
                ColumnCount = 2,
                Padding = new Padding(0)
            };
            btnPanel.ColumnStyles.Add(new ColumnStyle(SizeType.Percent, 50F));
            btnPanel.ColumnStyles.Add(new ColumnStyle(SizeType.Percent, 50F));

            btnInstall = new MaterialButton
            {
                Text = "Install Selected",
                Type = MaterialButton.MaterialButtonType.Contained,
                Enabled = false,
                Dock = DockStyle.Fill,
                Margin = new Padding(0, 0, 5, 0)
            };
            btnInstall.Click += BtnInstall_Click;
            btnPanel.Controls.Add(btnInstall, 0, 0);

            btnClose = new MaterialButton
            {
                Text = "Close",
                Type = MaterialButton.MaterialButtonType.Outlined,
                Dock = DockStyle.Fill,
                Margin = new Padding(5, 0, 0, 0)
            };
            btnClose.Click += (s, e) => this.Close();
            btnPanel.Controls.Add(btnClose, 1, 0);

            mainPanel.Controls.Add(btnPanel, 0, 3);
        }

        protected override async void OnLoad(EventArgs e)
        {
            base.OnLoad(e);
            await LoadReleasesAsync();
        }

        private async Task LoadReleasesAsync()
        {
            lblStatus.Text = "Loading releases from GitHub...";
            lstReleases.Items.Clear();
            btnInstall.Enabled = false;
            btnRefresh.Enabled = false;

            _releases = await UpdateChecker.GetAvailableReleasesAsync();

            if (_releases.Count == 0)
            {
                lblStatus.Text = "No releases found or network error.";
                btnRefresh.Enabled = true;
                return;
            }

            Version currentVersion = System.Reflection.Assembly.GetExecutingAssembly().GetName().Version ?? new Version(0, 0, 0, 0);

            foreach (var release in _releases.OrderByDescending(r => r.Version))
            {
                var item = new ListViewItem(release.Version.ToString());
                item.SubItems.Add(release.Channel.ToString());
                item.SubItems.Add(release.PublishedAt.ToString("yyyy-MM-dd HH:mm"));
                item.SubItems.Add(release.Name);
                
                if (release.Version == currentVersion)
                {
                    item.Text += " (Current)";
                    item.BackColor = Color.LightGreen;
                }

                item.Tag = release;
                lstReleases.Items.Add(item);
            }

            lblStatus.Text = $"Found {_releases.Count} releases. Current version: {currentVersion}";
            btnRefresh.Enabled = true;
        }

        private void CmbChannel_SelectedIndexChanged(object? sender, EventArgs e)
        {
            var selected = cmbChannel.SelectedIndex == 1 ? UpdateChannel.Beta : UpdateChannel.Production;
            if (AppSettings.Current.Channel != selected)
            {
                AppSettings.Current.Channel = selected;
                AppSettings.Save();
                lblStatus.Text = $"Auto-update channel changed to {selected}.";
            }
        }

        private void LstReleases_SelectedIndexChanged(object? sender, EventArgs e)
        {
            btnInstall.Enabled = lstReleases.SelectedItems.Count > 0;
        }

        private void BtnInstall_Click(object? sender, EventArgs e)
        {
            if (lstReleases.SelectedItems.Count == 0) return;

            var selectedRelease = lstReleases.SelectedItems[0].Tag as GitHubRelease;
            if (selectedRelease == null) return;

            Version currentVersion = System.Reflection.Assembly.GetExecutingAssembly().GetName().Version ?? new Version(0, 0, 0, 0);

            string action = selectedRelease.Version > currentVersion ? "upgrade" : 
                            selectedRelease.Version < currentVersion ? "downgrade" : "reinstall";

            var result = MessageBox.Show($"Are you sure you want to {action} to {selectedRelease.Version} ({selectedRelease.Channel})?",
                "Confirm Installation", MessageBoxButtons.YesNo, MessageBoxIcon.Question);

            if (result == DialogResult.Yes)
            {
                UpdateChecker.LaunchUpdaterAndExit(selectedRelease.DownloadUrl);
            }
        }
    }
}
